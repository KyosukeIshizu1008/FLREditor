mod app;
mod encoding;
mod record;
mod schema;
mod theme;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("FLR Editor — 固定長レコードエディタ"),
        ..Default::default()
    };

    eframe::run_native(
        "FLR Editor",
        native_options,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::FlrApp::new()))
        }),
    )
}

/// Install a CJK-capable font if one is available on the system, so Shift_JIS
/// decoded Japanese characters render instead of showing as tofu boxes.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let candidates: &[&str] = &[
        // macOS
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Linux (common Noto CJK locations)
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        // Windows
        "C:\\Windows\\Fonts\\YuGothM.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
    ];

    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };

        // Two copies of the same font: the proportional one untouched, and a
        // monospace one with a small upward y-shift so its baseline visually
        // matches Hack/Ubuntu Mono when those are present (egui mono-family
        // members are stacked, so we want every entry to render on the same
        // line as the row's primary font).
        fonts
            .font_data
            .insert("cjk".to_owned(), egui::FontData::from_owned(bytes.clone()));
        fonts.font_data.insert(
            "cjk_mono".to_owned(),
            egui::FontData::from_owned(bytes).tweak(egui::FontTweak {
                y_offset_factor: -0.02,
                ..Default::default()
            }),
        );

        // Insert at index 0 (highest priority) so Latin glyphs come from the
        // CJK font too — keeping every character on the same baseline. Without
        // this, Ubuntu-Light (egui's default proportional) would draw digits
        // while Hiragino drew kana, and the differing metrics produced the
        // visible up/down jitter the user reported.
        if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            prop.insert(0, "cjk".to_owned());
        }
        if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            mono.insert(0, "cjk_mono".to_owned());
        }
        log::info!("loaded CJK font: {path}");
        break;
    }

    ctx.set_fonts(fonts);
}
