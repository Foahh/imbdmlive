use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use windows::Win32::System::SystemInformation::GetLocalTime;

/// What kind of line a [`DanmakuLine`] is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Regular danmaku (bullet chat).
    Danmu,
    /// Gift / donation.
    Gift,
    /// Super Chat (醒目留言).
    SuperChat,
    /// Guard / membership purchase (上舰).
    Guard,
    /// User entered the live room.
    Enter,
    /// Local system / status message (not from the live room).
    System,
}

/// A single rendered line in the danmaku list.
#[derive(Clone)]
pub struct DanmakuLine {
    pub kind: LineKind,
    /// Local time when this line was received, formatted as `HH:MM:SS`.
    pub timestamp: String,
    /// Sender name (empty for [`LineKind::System`]).
    pub user: String,
    /// Message body / formatted gift text.
    pub text: String,
}

/// Mutable overlay state shared across threads as `Arc<Mutex<OverlayState>>`.
pub struct OverlayState {
    /// Recent lines, oldest first.
    pub lines: VecDeque<DanmakuLine>,
    /// High-energy rank count (from `ONLINE_RANK_COUNT`).
    pub rank_count: u64,
    /// Online viewer count (from `ONLINE_RANK_COUNT`).
    pub online_count: u64,
    /// Current room popularity.
    pub popularity: u64,
    /// Whether the websocket is believed to be connected.
    pub connected: bool,
    /// Room currently being displayed.
    pub room_id: String,
}

impl OverlayState {
    pub fn new(room_id: String) -> Self {
        Self {
            lines: VecDeque::new(),
            rank_count: 0,
            online_count: 0,
            popularity: 0,
            connected: false,
            room_id,
        }
    }

    /// Wrap in the shared handle used everywhere else.
    pub fn shared(room_id: String) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::new(room_id)))
    }

    fn timestamp() -> String {
        let t = unsafe { GetLocalTime() };
        format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
    }

    fn push(&mut self, line: DanmakuLine) {
        self.lines.push_back(line);
    }

    pub fn push_danmu(&mut self, user: String, text: String) {
        self.push(DanmakuLine {
            kind: LineKind::Danmu,
            timestamp: Self::timestamp(),
            user,
            text,
        });
    }

    pub fn push_gift(&mut self, user: String, gift: String, num: String) {
        self.push(DanmakuLine {
            kind: LineKind::Gift,
            timestamp: Self::timestamp(),
            user,
            text: format!("送出 {} × {}", gift, num),
        });
    }

    pub fn push_super_chat(&mut self, user: String, text: String) {
        self.push(DanmakuLine {
            kind: LineKind::SuperChat,
            timestamp: Self::timestamp(),
            user,
            text,
        });
    }

    pub fn push_guard(&mut self, user: String, text: String) {
        self.push(DanmakuLine {
            kind: LineKind::Guard,
            timestamp: Self::timestamp(),
            user,
            text,
        });
    }

    pub fn push_enter(&mut self, user: String) {
        self.push(DanmakuLine {
            kind: LineKind::Enter,
            timestamp: Self::timestamp(),
            user,
            text: "进入直播间".to_string(),
        });
    }

    pub fn push_system(&mut self, text: impl Into<String>) {
        self.push(DanmakuLine {
            kind: LineKind::System,
            timestamp: Self::timestamp(),
            user: String::new(),
            text: text.into(),
        });
    }

    pub fn set_online(&mut self, count: u64, online_count: u64) {
        self.rank_count = count;
        self.online_count = online_count;
    }

    pub fn set_popularity(&mut self, popularity: u64) {
        self.popularity = popularity;
    }

    /// Drop all lines and reset counters (used on reconnect to a new room).
    pub fn reset(&mut self, room_id: String) {
        self.lines.clear();
        self.rank_count = 0;
        self.online_count = 0;
        self.popularity = 0;
        self.connected = false;
        self.room_id = room_id;
    }
}
