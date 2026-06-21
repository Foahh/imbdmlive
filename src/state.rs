use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

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
    /// Local system / status message (not from the live room).
    System,
}

/// A single rendered line in the danmaku list.
#[derive(Clone)]
pub struct DanmakuLine {
    pub kind: LineKind,
    /// Sender name (empty for [`LineKind::System`]).
    pub user: String,
    /// Message body / formatted gift text.
    pub text: String,
}

/// Mutable overlay state shared across threads as `Arc<Mutex<OverlayState>>`.
pub struct OverlayState {
    /// Recent lines, oldest first, capped at `max_lines`.
    pub lines: VecDeque<DanmakuLine>,
    /// Max number of retained lines (mirrors `Config::max_lines`).
    pub max_lines: usize,
    /// High-energy rank count (from `ONLINE_RANK_COUNT`).
    pub rank_count: u64,
    /// Online viewer count (from `ONLINE_RANK_COUNT`).
    pub online_count: u64,
    /// Whether the websocket is believed to be connected.
    pub connected: bool,
    /// Room currently being displayed.
    pub room_id: String,
}

impl OverlayState {
    pub fn new(max_lines: usize, room_id: String) -> Self {
        Self {
            lines: VecDeque::with_capacity(max_lines.min(1024)),
            max_lines: max_lines.max(1),
            rank_count: 0,
            online_count: 0,
            connected: false,
            room_id,
        }
    }

    /// Wrap in the shared handle used everywhere else.
    pub fn shared(max_lines: usize, room_id: String) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::new(max_lines, room_id)))
    }

    fn push(&mut self, line: DanmakuLine) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn push_danmu(&mut self, user: String, text: String) {
        self.push(DanmakuLine { kind: LineKind::Danmu, user, text });
    }

    pub fn push_gift(&mut self, user: String, gift: String, num: String) {
        self.push(DanmakuLine {
            kind: LineKind::Gift,
            user,
            text: format!("送出 {} × {}", gift, num),
        });
    }

    pub fn push_super_chat(&mut self, user: String, text: String) {
        self.push(DanmakuLine { kind: LineKind::SuperChat, user, text });
    }

    pub fn push_guard(&mut self, user: String, text: String) {
        self.push(DanmakuLine { kind: LineKind::Guard, user, text });
    }

    pub fn push_system(&mut self, text: impl Into<String>) {
        self.push(DanmakuLine { kind: LineKind::System, user: String::new(), text: text.into() });
    }

    pub fn set_online(&mut self, count: u64, online_count: u64) {
        self.rank_count = count;
        self.online_count = online_count;
    }

    /// Drop all lines and reset counters (used on reconnect to a new room).
    pub fn reset(&mut self, room_id: String) {
        self.lines.clear();
        self.rank_count = 0;
        self.online_count = 0;
        self.connected = false;
        self.room_id = room_id;
    }
}
