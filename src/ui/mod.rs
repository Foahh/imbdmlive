use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use hudhook::imgui::{Condition, Context, Io, StyleColor, Ui, WindowFlags};
use hudhook::{ImguiRenderLoop, MessageFilter, RenderContext};

use crate::config::{Config, toggle_key_to_imgui};
use crate::state::{LineKind, OverlayState};

mod clipboard;
mod fonts;

/// Channel the UI uses to ask the supervisor to (re)connect with a new config.
pub type ReconnectSender = Sender<Config>;

// Line colors (RGBA).
const COL_HEADER: [f32; 4] = [0.55, 0.85, 1.0, 1.0];
const COL_NAME: [f32; 4] = [0.45, 0.78, 1.0, 1.0];
const COL_DANMU: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
const COL_GIFT: [f32; 4] = [1.0, 0.62, 0.25, 1.0];
const COL_SUPERCHAT: [f32; 4] = [1.0, 0.84, 0.10, 1.0];
const COL_GUARD: [f32; 4] = [0.40, 0.80, 1.0, 1.0];
const COL_ENTER: [f32; 4] = [0.55, 0.90, 0.55, 1.0];
const COL_SYSTEM: [f32; 4] = [0.60, 0.60, 0.60, 1.0];

fn body_color(kind: LineKind) -> [f32; 4] {
    match kind {
        LineKind::Danmu => COL_DANMU,
        LineKind::Gift => COL_GIFT,
        LineKind::SuperChat => COL_SUPERCHAT,
        LineKind::Guard => COL_GUARD,
        LineKind::Enter => COL_ENTER,
        LineKind::System => COL_SYSTEM,
    }
}

fn snap_to_tenth(value: f32, min: f32, max: f32) -> f32 {
    ((value * 10.0).round() / 10.0).clamp(min, max)
}

pub struct OverlayUi {
    state: Arc<Mutex<OverlayState>>,
    // `Sender` is `Send` but `!Sync`;
    // hudhook needs the render loop to be `Sync`, so guard it with a `Mutex`.
    reconnect_tx: Mutex<ReconnectSender>,
    cfg: Config,

    // Edit buffers for the config window.
    room_buf: String,
    cookies_buf: String,
    log_level_buf: String,

    // Shared with `message_filter` (which only gets `&self`).
    config_open: AtomicBool,
    toggle_key: hudhook::imgui::Key,
    font_loaded: bool,
    danmaku_at_bottom: bool,
    scroll_to_bottom_requested: bool,
}

impl OverlayUi {
    pub fn new(
        state: Arc<Mutex<OverlayState>>,
        reconnect_tx: ReconnectSender,
        cfg: Config,
    ) -> Self {
        let toggle_key = toggle_key_to_imgui(&cfg.toggle_key).unwrap_or(hudhook::imgui::Key::F8);
        Self {
            room_buf: cfg.room_id.clone(),
            cookies_buf: cfg.cookies.clone().unwrap_or_default(),
            log_level_buf: cfg.log_level.clone(),
            config_open: AtomicBool::new(false),
            toggle_key,
            font_loaded: false,
            danmaku_at_bottom: true,
            scroll_to_bottom_requested: false,
            state,
            reconnect_tx: Mutex::new(reconnect_tx),
            cfg,
        }
    }

    fn draw_danmaku(&mut self, ui: &Ui, config_open: bool) {
        // When the config window is open, let the user drag/resize and remember
        // the result; otherwise keep it pinned and non-interactive.
        let cond = if config_open {
            Condition::FirstUseEver
        } else {
            Condition::Always
        };
        let mut flags = WindowFlags::NO_TITLE_BAR
            | WindowFlags::NO_COLLAPSE
            | WindowFlags::NO_SAVED_SETTINGS
            | WindowFlags::NO_FOCUS_ON_APPEARING
            | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;
        if !config_open {
            flags |= WindowFlags::NO_MOVE
                | WindowFlags::NO_RESIZE
                | WindowFlags::NO_INPUTS
                | WindowFlags::NO_NAV;
        }

        let state = Arc::clone(&self.state);
        let mut new_geometry: Option<([f32; 2], [f32; 2])> = None;
        let show_jump_button = !self.danmaku_at_bottom;
        let force_scroll_to_bottom = self.scroll_to_bottom_requested;
        self.scroll_to_bottom_requested = false;
        let mut danmaku_at_bottom = self.danmaku_at_bottom || force_scroll_to_bottom;
        let mut jump_to_bottom_clicked = false;

        ui.window("BDMLive##danmaku")
            .position(self.cfg.pos, cond)
            .size(self.cfg.size, cond)
            .bg_alpha(self.cfg.opacity)
            .flags(flags)
            .build(|| {
                if let Ok(s) = state.lock() {
                    let status = if s.connected {
                        "已连接"
                    } else {
                        "未连接"
                    };
                    let status_text = format!(
                        "{status} | 房间 {} | 在线 {} | 人气 {}",
                        s.room_id, s.online_count, s.popularity
                    );
                    ui.align_text_to_frame_padding();
                    let _h = ui.push_style_color(StyleColor::Text, COL_HEADER);
                    ui.text(status_text);
                    drop(_h);

                    if show_jump_button {
                        let button_size = ui.frame_height();
                        let button_x = (ui.window_content_region_max()[0] - button_size)
                            .max(ui.cursor_pos()[0]);
                        ui.same_line_with_pos(button_x);
                        if ui.button_with_size("↓##jump-to-bottom", [button_size, 0.0]) {
                            jump_to_bottom_clicked = true;
                            danmaku_at_bottom = true;
                        }
                    }

                    ui.separator();

                    ui.child_window("##danmaku-lines")
                        .size([0.0, 0.0])
                        .border(false)
                        .scroll_bar(true)
                        .scrollable(true)
                        .build(|| {
                            let at_bottom = ui.scroll_y() >= ui.scroll_max_y() - 1.0;
                            danmaku_at_bottom = at_bottom || force_scroll_to_bottom;
                            for line in &s.lines {
                                if line.kind == LineKind::Enter {
                                    let _c = ui.push_style_color(StyleColor::Text, COL_SYSTEM);
                                    ui.text_wrapped(format!(
                                        "[{}] {}: {}",
                                        line.timestamp, line.user, line.text
                                    ));
                                } else if line.user.is_empty() {
                                    let _c = ui
                                        .push_style_color(StyleColor::Text, body_color(line.kind));
                                    ui.text_wrapped(format!("[{}] {}", line.timestamp, line.text));
                                } else {
                                    let _n = ui.push_style_color(StyleColor::Text, COL_NAME);
                                    ui.text(format!("[{}] {}:", line.timestamp, line.user));
                                    drop(_n);
                                    ui.same_line();
                                    let _c = ui
                                        .push_style_color(StyleColor::Text, body_color(line.kind));
                                    ui.text_wrapped(&line.text);
                                }
                            }
                            // Keep the newest line in view unless the user scrolled up.
                            if at_bottom || force_scroll_to_bottom {
                                ui.set_scroll_here_y_with_ratio(1.0);
                            }
                        });
                }

                if config_open {
                    new_geometry = Some((ui.window_pos(), ui.window_size()));
                }
            });

        if let Some((pos, size)) = new_geometry {
            self.cfg.pos = pos;
            self.cfg.size = size;
        }
        self.danmaku_at_bottom = danmaku_at_bottom;
        if jump_to_bottom_clicked {
            self.scroll_to_bottom_requested = true;
        }
    }

    fn draw_config(&mut self, ui: &Ui) {
        ui.window("BDMLive 设置##config")
            .size([360.0, 0.0], Condition::FirstUseEver)
            .position([16.0, 360.0], Condition::FirstUseEver)
            .collapsible(false)
            .build(|| {
                ui.input_text("房间号", &mut self.room_buf).build();
                ui.input_text("Cookies", &mut self.cookies_buf)
                    .password(false)
                    .build();
                ui.separator();

                if ui
                    .slider_config("不透明度", 0.0, 1.0)
                    .display_format("%.1f")
                    .build(&mut self.cfg.opacity)
                {
                    self.cfg.opacity = snap_to_tenth(self.cfg.opacity, 0.0, 1.0);
                }
                if ui
                    .slider_config("字号", 10.0, 48.0)
                    .display_format("%.1f")
                    .build(&mut self.cfg.font_size)
                {
                    self.cfg.font_size = snap_to_tenth(self.cfg.font_size, 10.0, 48.0);
                }
                ui.separator();

                if ui.button("保存") {
                    self.apply_and_save(true);
                }
                ui.same_line();
                if ui.button("关闭") {
                    self.config_open.store(false, Ordering::Relaxed);
                }

                ui.separator();
                let connected = self.state.lock().map(|s| s.connected).unwrap_or(false);
                ui.text(if connected {
                    "状态: 已连接"
                } else {
                    "状态: 未连接"
                });
                if ui.button("测试消息") {
                    self.fire_test_messages();
                }
                if !self.font_loaded {
                    let _c = ui.push_style_color(StyleColor::Text, COL_GIFT);
                    ui.text_wrapped("警告: 未能加载系统字体");
                }
            });
    }

    /// Push the edited values into `self.cfg`,
    /// persist to disk, and optionally request a reconnect.
    fn apply_and_save(&mut self, reconnect: bool) {
        self.cfg.room_id = self.room_buf.trim().to_string();
        let cookies = self.cookies_buf.trim();
        self.cfg.cookies = if cookies.is_empty() {
            None
        } else {
            Some(cookies.to_string())
        };
        self.cfg.log_level = self.log_level_buf.trim().to_ascii_lowercase();
        match crate::config::parse_log_level(&self.cfg.log_level) {
            Some(level) => crate::logger::set_level(level),
            None => {
                self.cfg.log_level = Config::default().log_level;
                self.log_level_buf = self.cfg.log_level.clone();
                crate::logger::set_level(self.cfg.log_level_filter());
                log::warn!("Invalid log level; using {}", self.cfg.log_level);
            }
        }

        if let Err(e) = self.cfg.save() {
            log::warn!("Failed to save config: {e}");
        }

        if reconnect {
            if let Ok(mut s) = self.state.lock() {
                if s.room_id != self.cfg.room_id {
                    s.reset(self.cfg.room_id.clone());
                }
            }
            let sent = self
                .reconnect_tx
                .lock()
                .map(|tx| tx.send(self.cfg.clone()).is_ok())
                .unwrap_or(false);
            if !sent {
                log::warn!("Reconnect channel closed");
            }
        }
    }

    fn fire_test_messages(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            for i in 1..=80 {
                match i % 6 {
                    0 => s.push_danmu(
                        format!("测试用户{i}"),
                        format!("第 {i} 条普通弹幕，用于测试滚动区域"),
                    ),
                    1 => s.push_gift(
                        format!("测试礼物{i}"),
                        "辣条".to_string(),
                        (i % 10 + 1).to_string(),
                    ),
                    2 => s.push_super_chat(
                        format!("测试醒目留言{i}"),
                        format!("第 {i} 条醒目留言，用于测试滚动定位"),
                    ),
                    3 => s.push_guard(format!("测试舰长{i}"), "开通舰长".to_string()),
                    4 => s.push_enter(format!("测试进场{i}")),
                    _ => s.push_system(format!("第 {i} 条系统消息，用于测试滚动行为")),
                };
            }
        }
    }
}

impl ImguiRenderLoop for OverlayUi {
    fn initialize<'a>(&'a mut self, ctx: &mut Context, _rc: &'a mut dyn RenderContext) {
        ctx.set_clipboard_backend(clipboard::WindowsClipboard);
        self.font_loaded = fonts::add_system_fonts(ctx, self.cfg.font_size);
    }

    fn render(&mut self, ui: &mut Ui) {
        if ui.is_key_pressed(self.toggle_key) {
            let open = !self.config_open.load(Ordering::Relaxed);
            self.config_open.store(open, Ordering::Relaxed);
        }
        let config_open = self.config_open.load(Ordering::Relaxed);

        self.draw_danmaku(ui, config_open);
        if config_open {
            self.draw_config(ui);
        }
    }

    fn message_filter(&self, _io: &Io) -> MessageFilter {
        // Only steal input from the game while the config window is open.
        if self.config_open.load(Ordering::Relaxed) {
            MessageFilter::InputAll
        } else {
            MessageFilter::empty()
        }
    }
}
