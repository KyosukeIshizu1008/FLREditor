//! Visual theme — palette, rounding, spacing, typography.
//!
//! All colour and spacing decisions live here so individual UI modules only
//! reference semantic names (`ACCENT`, `WARN`, `BG_PANEL`, …) rather than raw
//! `Color32` literals. Tweak one constant, the whole app shifts.

#![allow(dead_code)] // palette includes colours reserved for future use

use eframe::egui::{
    self,
    style::{Selection, Visuals, Widgets},
    Color32, FontFamily, FontId, Margin, Rounding, Stroke, TextStyle,
};

// ── Brand / semantic colours ─────────────────────────────────────────────────
pub const ACCENT: Color32 = Color32::from_rgb(0x5B, 0xC0, 0xBE); // teal
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x35, 0x76, 0x76); // dark teal
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x22, 0x48, 0x4A); // very dark teal
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0xB2, 0x59); // warm amber
pub const ERR: Color32 = Color32::from_rgb(0xE8, 0x6A, 0x6A); // muted red
pub const SUCCESS: Color32 = Color32::from_rgb(0x7A, 0xC7, 0x4F); // green

// ── Backgrounds (3-step hierarchy) ──────────────────────────────────────────
pub const BG_BASE: Color32 = Color32::from_rgb(0x16, 0x19, 0x1F); // outermost
pub const BG_PANEL: Color32 = Color32::from_rgb(0x1D, 0x21, 0x29); // panels / central
pub const BG_RAISED: Color32 = Color32::from_rgb(0x25, 0x2A, 0x33); // buttons, inputs
pub const BG_HOVER: Color32 = Color32::from_rgb(0x2E, 0x34, 0x3E);
pub const BG_STRIPE: Color32 = Color32::from_rgb(0x20, 0x24, 0x2C); // table striping

// ── Foregrounds ─────────────────────────────────────────────────────────────
pub const FG_TEXT: Color32 = Color32::from_rgb(0xE4, 0xE6, 0xEB);
pub const FG_MUTED: Color32 = Color32::from_rgb(0x8A, 0x90, 0x9C);
pub const BORDER: Color32 = Color32::from_rgb(0x36, 0x3C, 0x46);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x4A, 0x52, 0x5E);

/// Install the theme into the egui context. Call once after `set_fonts`.
pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // ── Typography ──────────────────────────────────────────────────────────
    let mut text_styles = std::collections::BTreeMap::new();
    text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
    text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
    text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
    text_styles.insert(TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional));
    text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.5, FontFamily::Monospace),
    );
    style.text_styles = text_styles;

    // ── Visuals (dark, with our palette) ────────────────────────────────────
    let mut v = Visuals::dark();
    v.override_text_color = Some(FG_TEXT);
    v.window_fill = BG_PANEL;
    v.panel_fill = BG_PANEL;
    v.faint_bg_color = BG_STRIPE;
    v.extreme_bg_color = BG_BASE;
    v.code_bg_color = BG_BASE;
    v.hyperlink_color = ACCENT;

    v.selection = Selection {
        bg_fill: ACCENT_SOFT,
        stroke: Stroke::new(1.0, ACCENT),
    };

    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_rounding = Rounding::same(8.0);
    v.menu_rounding = Rounding::same(6.0);
    v.popup_shadow.color = Color32::from_black_alpha(96);

    // Widgets: rounded corners, accent-on-hover/active
    let mut w = Widgets::default();
    let r = Rounding::same(6.0);

    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, FG_TEXT);
    w.noninteractive.rounding = r;

    w.inactive.bg_fill = BG_RAISED;
    w.inactive.weak_bg_fill = BG_RAISED;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, FG_TEXT);
    w.inactive.rounding = r;

    w.hovered.bg_fill = BG_HOVER;
    w.hovered.weak_bg_fill = BG_HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    w.hovered.fg_stroke = Stroke::new(1.0, FG_TEXT);
    w.hovered.rounding = r;

    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.active.rounding = r;

    w.open.bg_fill = BG_HOVER;
    w.open.weak_bg_fill = BG_HOVER;
    w.open.bg_stroke = Stroke::new(1.0, ACCENT);
    w.open.fg_stroke = Stroke::new(1.0, FG_TEXT);
    w.open.rounding = r;

    v.widgets = w;
    style.visuals = v;

    // ── Spacing ─────────────────────────────────────────────────────────────
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.menu_margin = Margin::symmetric(8.0, 6.0);
    style.spacing.window_margin = Margin::same(8.0);
    style.spacing.indent = 16.0;
    style.spacing.scroll.bar_width = 10.0;
    style.spacing.scroll.bar_outer_margin = 2.0;

    ctx.set_style(style);
}

/// Render a small accent strip + heading text, used at the top of major panes.
pub fn section_heading(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _resp) =
            ui.allocate_exact_size(egui::vec2(3.0, 16.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, Rounding::same(1.5), ACCENT);
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(text)
                .strong()
                .color(FG_TEXT)
                .size(14.0),
        );
    });
}
