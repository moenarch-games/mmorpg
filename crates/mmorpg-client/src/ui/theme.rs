//! The shared interface theme (`content/ui/theme.json`, parsed by
//! `mmorpg_client_model::theme`) as egui visuals, so the native window wears
//! the browser's colours, radii, spacing and text sizes.
use egui::{Color32, CornerRadius, FontId, Margin, Stroke, Style, TextStyle, Vec2};
use mmorpg_client_model::theme::{Rgba, Theme};
use std::sync::OnceLock;

/// The shared theme, parsed once.
pub fn shared() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(Theme::shared)
}

#[must_use]
pub fn color(rgba: Rgba) -> Color32 {
    let [r, g, b, a] = rgba.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn radius(pixels: f32) -> CornerRadius {
    CornerRadius::same(pixels.round().clamp(0.0, 255.0) as u8)
}

/// Dresses every egui style of `ctx` in the shared theme.
pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| dress(style, shared()));
}

fn dress(style: &mut Style, theme: &Theme) {
    let c = &theme.color;
    let text = &theme.text;
    for (kind, size) in [
        (TextStyle::Small, text.small),
        (TextStyle::Body, text.body),
        (TextStyle::Button, text.body),
        (TextStyle::Heading, text.heading),
    ] {
        style.text_styles.insert(kind, FontId::proportional(size));
    }
    let space = &theme.space;
    style.spacing.item_spacing = Vec2::new(space.sm, space.sm * 0.75);
    style.spacing.button_padding = Vec2::new(space.md, space.xs + 2.0);
    style.spacing.window_margin = Margin::same(space.md as i8);

    let visuals = &mut style.visuals;
    visuals.dark_mode = true;
    visuals.panel_fill = color(c.panel);
    visuals.window_fill = color(c.panel);
    visuals.window_stroke = Stroke::new(1.0, color(c.border));
    visuals.window_corner_radius = radius(theme.radius.panel);
    visuals.menu_corner_radius = radius(theme.radius.control);
    visuals.extreme_bg_color = color(c.panel_raised);
    visuals.faint_bg_color = color(c.panel_raised);
    visuals.hyperlink_color = color(c.gold_bright);
    visuals.selection.bg_fill = color(c.gold).gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, color(c.gold_bright));

    let control = radius(theme.radius.control);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, color(c.text));
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, color(c.border));
    widgets.noninteractive.corner_radius = control;
    for (state, fill, border, ink) in [
        (&mut widgets.inactive, c.panel_raised, c.border, c.text),
        (
            &mut widgets.hovered,
            c.accent_fill,
            c.border_strong,
            c.text_heading,
        ),
        (&mut widgets.active, c.accent_fill, c.gold, c.gold_bright),
        (&mut widgets.open, c.accent_fill, c.border_strong, c.text),
    ] {
        state.bg_fill = color(fill);
        state.weak_bg_fill = color(fill);
        state.bg_stroke = Stroke::new(1.0, color(border));
        state.fg_stroke = Stroke::new(1.0, color(ink));
        state.corner_radius = control;
        state.expansion = 0.0;
    }
}

/// A panel on the screen: the theme's panel fill, border and corners.
pub fn panel() -> egui::Frame {
    let theme = shared();
    egui::Frame::new()
        .fill(color(theme.color.panel))
        .stroke(Stroke::new(1.0, color(theme.color.border)))
        .corner_radius(radius(theme.radius.panel))
        .inner_margin(Margin::same(theme.space.md as i8))
}

/// The screen's primary action: gold, large, one per screen.
pub fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let theme = shared();
    let button = egui::Button::new(
        egui::RichText::new(text)
            .size(theme.text.heading)
            .color(color(theme.color.gold_ink)),
    )
    .fill(color(theme.color.gold))
    .stroke(Stroke::new(1.0, color(theme.color.gold_bright)))
    .min_size(Vec2::new(220.0, 48.0));
    ui.add_enabled(enabled, button)
}

/// A muted caption.
#[must_use]
pub fn muted(text: impl Into<String>) -> egui::RichText {
    let theme = shared();
    egui::RichText::new(text)
        .size(theme.text.small)
        .color(color(theme.color.text_muted))
}

/// A section or screen heading.
#[must_use]
pub fn heading(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text)
        .size(size)
        .color(color(shared().color.text_heading))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_theme_dresses_egui() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let style = ctx.style_of(egui::Theme::Dark);
        let gold = color(shared().color.gold);
        assert_eq!(style.visuals.window_fill, color(shared().color.panel));
        assert_eq!(style.visuals.widgets.active.bg_stroke.color, gold);
        assert_eq!(
            style.text_styles[&TextStyle::Heading].size,
            shared().text.heading
        );
    }
}
