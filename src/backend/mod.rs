use hudhook::Hudhook;
use hudhook::hooks::dx9::ImguiDx9Hooks;
use hudhook::windows::Win32::Foundation::HINSTANCE;

use crate::ui::OverlayUi;

/// Installs the overlay render loop into a host process.
pub trait RenderBackend {
    /// Hook the host's present path and take ownership of `ui`. `hmodule`.
    fn install(&self, ui: OverlayUi, hmodule: Option<HINSTANCE>) -> Result<(), String>;
}

pub struct Dx9Backend;

impl RenderBackend for Dx9Backend {
    fn install(&self, ui: OverlayUi, hmodule: Option<HINSTANCE>) -> Result<(), String> {
        let mut builder = Hudhook::builder().with::<ImguiDx9Hooks>(ui);
        if let Some(h) = hmodule {
            builder = builder.with_hmodule(h);
        }
        builder
            .build()
            .apply()
            .map_err(|e| format!("failed to apply D3D9 hooks: {e:?}"))
    }
}

/// Placeholder backends for future.
#[allow(dead_code)]
pub mod stub {
    use super::*;

    macro_rules! unsupported_backend {
        ($name:ident, $what:literal) => {
            #[doc = concat!("Stub backend for ", $what, " (not yet implemented).")]
            pub struct $name;
            impl RenderBackend for $name {
                fn install(
                    &self,
                    _ui: OverlayUi,
                    _hmodule: Option<HINSTANCE>,
                ) -> Result<(), String> {
                    Err(concat!($what, " backend is not implemented yet").to_string())
                }
            }
        };
    }

    unsupported_backend!(Dx11Backend, "Direct3D 11");
    unsupported_backend!(OpenGlBackend, "OpenGL 3");
}
