use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use blivedm::client::models::BiliMessage;
use blivedm::client::websocket::BiliLiveClient;
use futures::channel::mpsc::{self, TryRecvError};

use crate::config::Config;
use crate::state::{DanmakuLine, LineKind, OverlayState};

/// Handle to a running danmaku connection. Stops its threads on drop.
pub struct DanmakuHandle {
    stop: Arc<AtomicBool>,
}

impl DanmakuHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for DanmakuHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Connect to `cfg.room_id` and start streaming danmaku into `state`.
pub fn start(cfg: &Config, state: Arc<Mutex<OverlayState>>) -> DanmakuHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let room_id = cfg.room_id.clone();
    let cookies = cfg.cookies.clone();

    {
        let state = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("bdmlive-danmaku".into())
            .spawn(move || run(room_id, cookies, state, stop))
            .ok();
    }

    DanmakuHandle { stop }
}

fn set_connected(state: &Arc<Mutex<OverlayState>>, connected: bool) {
    if let Ok(mut s) = state.lock() {
        s.connected = connected;
    }
}

fn system(state: &Arc<Mutex<OverlayState>>, text: impl Into<String>) {
    if let Ok(mut s) = state.lock() {
        s.push_system(text);
    }
}

fn run(
    room_id: String,
    cookies: Option<String>,
    state: Arc<Mutex<OverlayState>>,
    stop: Arc<AtomicBool>,
) {
    let room_id = room_id.trim().to_string();
    if room_id.trim().parse::<u64>().is_err() {
        log::warn!("Invalid room id: {room_id:?}");
        system(&state, format!("无效房间号: {room_id}"));
        set_connected(&state, false);
        return;
    }

    let (tx, mut rx) = mpsc::channel::<BiliMessage>(64);

    let mut client = match BiliLiveClient::new_auto(cookies.as_deref(), &room_id, tx) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to connect to room {room_id}: {e}");
            system(&state, format!("连接失败: {e}"));
            set_connected(&state, false);
            return;
        }
    };
    client.send_auth();
    client.send_heart_beat();

    let client = Arc::new(Mutex::new(client));
    set_connected(&state, true);
    system(&state, format!("已连接房间 {room_id}"));
    log::info!("Connected to room {room_id}");
    let mut history = open_history(&room_id);

    // Heartbeat thread
    // polling `stop` frequently so reconnect is responsive.
    {
        let client = Arc::clone(&client);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut last = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                if last.elapsed() >= Duration::from_secs(20) {
                    if let Ok(mut c) = client.lock() {
                        c.send_heart_beat();
                    }
                    last = Instant::now();
                }
                thread::sleep(Duration::from_millis(250));
            }
        });
    }

    // Receive thread
    // pull frames off the socket.
    {
        let client = Arc::clone(&client);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if let Ok(mut c) = client.lock() {
                    if let Err(e) = c.receive() {
                        log::debug!("receive: {e}");
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
    }

    // Drain parsed messages into the overlay state on this thread.
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match rx.try_recv() {
            Ok(msg) => apply(msg, &state, &room_id, history.as_mut()),
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
            Err(TryRecvError::Closed) => break, // sender dropped
        }
    }

    set_connected(&state, false);
    log::info!("Danmaku worker for room {room_id} stopped");
}

/// Map one [`BiliMessage`] into the overlay state.
fn apply(
    msg: BiliMessage,
    state: &Arc<Mutex<OverlayState>>,
    room_id: &str,
    history: Option<&mut File>,
) {
    let Ok(mut s) = state.lock() else { return };
    if s.room_id.trim() != room_id {
        return;
    }
    match msg {
        BiliMessage::Danmu { user, text } => {
            let line = s.push_danmu(user, text);
            if let Some(file) = history {
                append_history(file, &line);
            }
        }
        BiliMessage::Gift { user, gift, num } => {
            let line = s.push_gift(user, gift, num);
            if let Some(file) = history {
                append_history(file, &line);
            }
        }
        BiliMessage::OnlineRankCount {
            count,
            online_count,
        } => s.set_online(count, online_count),
        // blivedm has no typed variant for these; parse from the raw JSON.
        BiliMessage::Raw(v) => apply_raw(&v, &mut s, history),
        _ => {}
    }
}

fn apply_raw(v: &serde_json::Value, s: &mut OverlayState, history: Option<&mut File>) {
    let cmd = v
        .get("cmd")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let data = v.get("data");
    match cmd {
        // User interaction. msg_type == 1 is a room-enter event.
        "INTERACT_WORD" | "INTERACT_WORD_V2" => {
            let msg_type = data.and_then(|d| d.get("msg_type")).and_then(as_u64);
            if msg_type == Some(1) {
                if let Some(user) = data.and_then(user_name_from_data) {
                    let line = s.push_enter(user);
                    if let Some(file) = history {
                        append_history(file, &line);
                    }
                }
            }
        }
        // Watched/popularity counter shown by the room.
        "WATCHED_CHANGE" => {
            if let Some(popularity) = data.and_then(|d| d.get("num")).and_then(as_u64) {
                s.set_popularity(popularity);
            }
        }
        // Super Chat (醒目留言).
        "SUPER_CHAT_MESSAGE" => {
            let user = data
                .and_then(|d| d.get("user_info"))
                .and_then(|u| u.get("uname"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let text = data
                .and_then(|d| d.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            if !text.is_empty() {
                let line = s.push_super_chat(user, text);
                if let Some(file) = history {
                    append_history(file, &line);
                }
            }
        }
        // Guard / membership purchase (上舰).
        "GUARD_BUY" => {
            let user = data
                .and_then(|d| d.get("username"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let gift = data
                .and_then(|d| d.get("gift_name"))
                .and_then(|n| n.as_str())
                .unwrap_or("舰长")
                .to_string();
            let line = s.push_guard(user, format!("开通 {gift}"));
            if let Some(file) = history {
                append_history(file, &line);
            }
        }
        _ => {}
    }
}

fn history_path(room_id: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("history")
        .join(format!("{room_id}.log"))
}

fn open_history(room_id: &str) -> Option<File> {
    let path = history_path(room_id);
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!(
                "Failed to create danmaku history directory {}: {e}",
                dir.display()
            );
            return None;
        }
    }

    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            log::info!("Danmaku history -> {}", path.display());
            Some(file)
        }
        Err(e) => {
            log::warn!("Failed to open danmaku history {}: {e}", path.display());
            None
        }
    }
}

fn append_history(file: &mut File, line: &DanmakuLine) {
    let kind = match line.kind {
        LineKind::Danmu => "DANMU",
        LineKind::Gift => "GIFT",
        LineKind::SuperChat => "SUPER_CHAT",
        LineKind::Guard => "GUARD",
        LineKind::Enter => "ENTER",
        LineKind::System => "SYSTEM",
    };
    let text = if line.user.is_empty() {
        format!("[{}] {:<10} {}\n", line.timestamp, kind, line.text)
    } else {
        format!(
            "[{}] {:<10} {}: {}\n",
            line.timestamp, kind, line.user, line.text
        )
    };

    if let Err(e) = file.write_all(text.as_bytes()) {
        log::warn!("Failed to write danmaku history: {e}");
    }
}

fn as_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn user_name_from_data(data: &serde_json::Value) -> Option<String> {
    data.get("uname")
        .and_then(|n| n.as_str())
        .or_else(|| data.get("username").and_then(|n| n.as_str()))
        .or_else(|| {
            data.get("uinfo")
                .and_then(|u| u.get("base"))
                .and_then(|b| b.get("name"))
                .and_then(|n| n.as_str())
        })
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
}
