use imgui::{Context, FontConfig, FontGlyphRanges, FontSource};

const SYMBOL_GLYPH_RANGES: &[u32] = &[
    0x2000, 0x206F, // General punctuation
    0x2070, 0x209F, // Superscripts and subscripts
    0x20A0, 0x20CF, // Currency symbols
    0x2100, 0x214F, // Letterlike symbols
    0x2150, 0x218F, // Number forms
    0x2190, 0x21FF, // Arrows
    0x2200, 0x22FF, // Mathematical operators
    0x2300, 0x23FF, // Miscellaneous technical
    0x2400, 0x243F, // Control pictures
    0x2440, 0x245F, // Optical character recognition
    0x2460, 0x24FF, // Enclosed alphanumerics
    0x2500, 0x257F, // Box drawing
    0x2580, 0x259F, // Block elements
    0x25A0, 0x25FF, // Geometric shapes, including U+25CB/U+25CF
    0x2600, 0x26FF, // Miscellaneous symbols
    0x2700, 0x27BF, // Dingbats
    0x27C0, 0x27EF, // Miscellaneous mathematical symbols-A
    0x27F0, 0x27FF, // Supplemental arrows-A
    0x2900, 0x297F, // Supplemental arrows-B
    0x2980, 0x29FF, // Miscellaneous mathematical symbols-B
    0x2A00, 0x2AFF, // Supplemental mathematical operators
    0x2B00, 0x2BFF, // Miscellaneous symbols and arrows
    0,
];

const EMOJI_GLYPH_RANGES: &[u32] = &[
    0x00A9, 0x00A9, 0x00AE, 0x00AE, 0x203C, 0x203C, 0x2049, 0x2049, 0x2122, 0x2122, 0x2139, 0x2139,
    0x2194, 0x21AA, 0x231A, 0x231B, 0x2328, 0x2328, 0x23CF, 0x23CF, 0x23E9, 0x23F3, 0x23F8, 0x23FA,
    0x24C2, 0x24C2, 0x25AA, 0x25AB, 0x25B6, 0x25B6, 0x25C0, 0x25C0, 0x25FB, 0x25FE, 0x2600, 0x27BF,
    0x2934, 0x2935, 0x2B05, 0x2B55, 0x3030, 0x3030, 0x303D, 0x303D, 0x3297, 0x3299, 0xFE00, 0xFE0F,
    0x1F000, 0x1FBFF, 0,
];

struct LoadedSystemFont {
    name: &'static str,
    bytes: Vec<u8>,
    glyph_ranges: FontGlyphRanges,
}

pub fn add_system_fonts(ctx: &mut Context, size_pixels: f32) -> bool {
    let loaded_fonts = system_fonts();
    if loaded_fonts.is_empty() {
        log::warn!("Could not load any configured system font candidates");
        ctx.fonts()
            .add_font(&[FontSource::DefaultFontData { config: None }]);
        return false;
    }

    let font_sources = loaded_fonts
        .iter()
        .map(|font| FontSource::TtfData {
            data: &font.bytes,
            size_pixels,
            config: Some(FontConfig {
                glyph_ranges: font.glyph_ranges.clone(),
                name: Some(font.name.to_string()),
                ..FontConfig::default()
            }),
        })
        .collect::<Vec<_>>();

    ctx.fonts().tex_desired_width = 4096;
    ctx.fonts().add_font(&font_sources);
    true
}

fn read_first_font(
    name: &'static str,
    paths: &[&str],
    glyph_ranges: FontGlyphRanges,
) -> Option<LoadedSystemFont> {
    for path in paths {
        match std::fs::read(path) {
            Ok(bytes) => {
                log::info!("Loaded system font {name}: {path}");
                return Some(LoadedSystemFont {
                    name,
                    bytes,
                    glyph_ranges,
                });
            }
            Err(e) => log::debug!("Could not read system font {path}: {e}"),
        }
    }
    None
}

fn system_fonts() -> Vec<LoadedSystemFont> {
    let mut fonts = Vec::new();

    if let Some(font) = read_first_font(
        "Microsoft YaHei",
        &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simsun.ttc",
            r"C:\Windows\Fonts\NotoSansSC-VF.ttf",
        ],
        FontGlyphRanges::chinese_full(),
    ) {
        fonts.push(font);
    }

    if let Some(font) = read_first_font(
        "Segoe UI Symbol",
        &[
            r"C:\Windows\Fonts\seguisym.ttf",
            r"C:\Windows\Fonts\symbol.ttf",
            r"C:\Windows\Fonts\SegUIVar.ttf",
        ],
        FontGlyphRanges::from_slice(SYMBOL_GLYPH_RANGES),
    ) {
        fonts.push(font);
    }

    if let Some(font) = read_first_font(
        "Segoe UI Emoji",
        &[r"C:\Windows\Fonts\seguiemj.ttf"],
        FontGlyphRanges::from_slice(EMOJI_GLYPH_RANGES),
    ) {
        fonts.push(font);
    }

    if let Some(font) = read_first_font(
        "Malgun Gothic",
        &[r"C:\Windows\Fonts\malgun.ttf"],
        FontGlyphRanges::korean(),
    ) {
        fonts.push(font);
    }

    if let Some(font) = read_first_font(
        "Japanese UI",
        &[
            r"C:\Windows\Fonts\meiryo.ttc",
            r"C:\Windows\Fonts\YuGothR.ttc",
            r"C:\Windows\Fonts\msgothic.ttc",
        ],
        FontGlyphRanges::japanese(),
    ) {
        fonts.push(font);
    }

    fonts
}
