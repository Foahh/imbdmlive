use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use hudhook::imgui::{Condition, Context, Io, StyleColor, Ui, WindowFlags};
use hudhook::{ImguiRenderLoop, MessageFilter, RenderContext};

use crate::config::{Config, toggle_key_to_imgui};
use crate::state::{LineKind, OverlayState};

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
const COL_SYSTEM: [f32; 4] = [0.60, 0.60, 0.60, 1.0];

fn body_color(kind: LineKind) -> [f32; 4] {
    match kind {
        LineKind::Danmu => COL_DANMU,
        LineKind::Gift => COL_GIFT,
        LineKind::SuperChat => COL_SUPERCHAT,
        LineKind::Guard => COL_GUARD,
        LineKind::System => COL_SYSTEM,
    }
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
    max_lines_edit: i32,

    // Shared with `message_filter` (which only gets `&self`).
    config_open: AtomicBool,
    toggle_key: hudhook::imgui::Key,
    font_loaded: bool,
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
            max_lines_edit: cfg.max_lines as i32,
            config_open: AtomicBool::new(false),
            toggle_key,
            font_loaded: false,
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
            | WindowFlags::NO_SCROLLBAR
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
                    let _h = ui.push_style_color(StyleColor::Text, COL_HEADER);
                    ui.text(format!(
                        "{status} | 房间 {} | 在线 {}",
                        s.room_id, s.online_count
                    ));
                    drop(_h);
                    ui.separator();

                    let at_bottom = ui.scroll_y() >= ui.scroll_max_y() - 1.0;
                    for line in &s.lines {
                        if line.user.is_empty() {
                            let _c = ui.push_style_color(StyleColor::Text, body_color(line.kind));
                            ui.text_wrapped(format!("[{}] {}", line.timestamp, line.text));
                        } else {
                            let _n = ui.push_style_color(StyleColor::Text, COL_NAME);
                            ui.text(format!("[{}] {}:", line.timestamp, line.user));
                            drop(_n);
                            ui.same_line();
                            let _c = ui.push_style_color(StyleColor::Text, body_color(line.kind));
                            ui.text_wrapped(&line.text);
                        }
                    }
                    // Keep the newest line in view unless the user scrolled up.
                    if at_bottom {
                        ui.set_scroll_here_y_with_ratio(1.0);
                    }
                }

                if config_open {
                    new_geometry = Some((ui.window_pos(), ui.window_size()));
                }
            });

        if let Some((pos, size)) = new_geometry {
            self.cfg.pos = pos;
            self.cfg.size = size;
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

                ui.slider("不透明度", 0.0, 1.0, &mut self.cfg.opacity);
                ui.slider("字号", 10.0, 48.0, &mut self.cfg.font_size);
                ui.slider("最大行数", 10, 1000, &mut self.max_lines_edit);
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
        self.cfg.max_lines = self.max_lines_edit.max(1) as usize;

        // Apply live visual settings immediately.
        if let Ok(mut s) = self.state.lock() {
            s.max_lines = self.cfg.max_lines;
            while s.lines.len() > s.max_lines {
                s.lines.pop_front();
            }
        }

        if let Err(e) = self.cfg.save() {
            log::warn!("Failed to save config: {e}");
        }

        if reconnect {
            if let Ok(mut s) = self.state.lock() {
                s.reset(self.cfg.room_id.clone());
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
}

impl ImguiRenderLoop for OverlayUi {
    fn initialize<'a>(&'a mut self, ctx: &mut Context, _rc: &'a mut dyn RenderContext) {
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
