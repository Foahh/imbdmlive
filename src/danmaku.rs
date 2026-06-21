use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use blivedm::client::models::BiliMessage;
use blivedm::client::websocket::BiliLiveClient;
use futures::channel::mpsc::{self, TryRecvError};

use crate::config::Config;
use crate::state::OverlayState;

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
            Ok(msg) => apply(msg, &state),
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
            Err(TryRecvError::Closed) => break, // sender dropped
        }
    }

    set_connected(&state, false);
    log::info!("Danmaku worker for room {room_id} stopped");
}

/// Map one [`BiliMessage`] into the overlay state.
fn apply(msg: BiliMessage, state: &Arc<Mutex<OverlayState>>) {
    let Ok(mut s) = state.lock() else { return };
    match msg {
        BiliMessage::Danmu { user, text } => s.push_danmu(user, text),
        BiliMessage::Gift { user, gift, num } => s.push_gift(user, gift, num),
        BiliMessage::OnlineRankCount { count, online_count } => s.set_online(count, online_count),
        // blivedm has no typed variant for these; parse from the raw JSON.
        BiliMessage::Raw(v) => apply_raw(&v, &mut s),
        _ => {}
    }
}

fn apply_raw(v: &serde_json::Value, s: &mut OverlayState) {
    let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
    let data = v.get("data");
    match cmd {
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
                s.push_super_chat(user, text);
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
            s.push_guard(user, format!("开通 {gift}"));
        }
        _ => {}
    }
}
