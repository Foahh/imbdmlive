use std::sync::{Arc, Mutex};

use imgui::{Condition, Context, StyleColor, Ui, WindowFlags};

use crate::config::Config;
use crate::state::{LineKind, OverlayState};

mod fonts;

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

/// Locked, non-interactive flags: the overlay never steals input or moves.
const WINDOW_FLAGS: WindowFlags = WindowFlags::NO_TITLE_BAR
    .union(WindowFlags::NO_COLLAPSE)
    .union(WindowFlags::NO_SAVED_SETTINGS)
    .union(WindowFlags::NO_FOCUS_ON_APPEARING)
    .union(WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS)
    .union(WindowFlags::NO_MOVE)
    .union(WindowFlags::NO_RESIZE)
    .union(WindowFlags::NO_INPUTS)
    .union(WindowFlags::NO_NAV);

pub struct OverlayUi {
    state: Arc<Mutex<OverlayState>>,
    cfg: Config,
    font_loaded: bool,
}

impl OverlayUi {
    pub fn new(state: Arc<Mutex<OverlayState>>, cfg: Config) -> Self {
        Self { state, cfg, font_loaded: false }
    }

    /// Build the font atlas. Called once during pipeline construction.
    pub fn initialize(&mut self, ctx: &mut Context) {
        self.font_loaded = fonts::add_system_fonts(ctx, self.cfg.font_size);
        if !self.font_loaded {
            log::warn!("System fonts unavailable; using imgui default font");
        }
        log::info!("Overlay UI initialized");
    }

    /// Draw one frame. `display_size` is the host backbuffer size, used to
    /// resolve the configured anchor into a window position.
    pub fn render(&mut self, ui: &Ui, display_size: [f32; 2]) {
        let pos = self.cfg.anchor.window_pos(display_size, self.cfg.size, self.cfg.offset);
        let state = Arc::clone(&self.state);

        ui.window("BDMLive##danmaku")
            .position(pos, Condition::Always)
            .size(self.cfg.size, Condition::Always)
            .bg_alpha(self.cfg.opacity)
            .flags(WINDOW_FLAGS)
            .build(|| {
                let Ok(s) = state.lock() else { return };

                let status = if s.connected { "已连接" } else { "未连接" };
                let status_text = format!(
                    "{status} | 房间 {} | 在线 {} | 人气 {}",
                    s.room_id, s.online_count, s.popularity
                );
                let _h = ui.push_style_color(StyleColor::Text, COL_HEADER);
                ui.text(status_text);
                drop(_h);
                ui.separator();

                ui.child_window("##danmaku-lines").size([0.0, 0.0]).border(false).build(|| {
                    let at_bottom = ui.scroll_y() >= ui.scroll_max_y() - 1.0;
                    for line in &s.lines {
                        if line.kind == LineKind::Enter {
                            let _c = ui.push_style_color(StyleColor::Text, COL_SYSTEM);
                            ui.text_wrapped(format!(
                                "[{}] {}: {}",
                                line.timestamp, line.user, line.text
                            ));
                        } else if line.user.is_empty() {
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
                    // Keep the newest line in view.
                    if at_bottom {
                        ui.set_scroll_here_y_with_ratio(1.0);
                    }
                });
            });
    }
}
