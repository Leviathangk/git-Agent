//! Shared desktop appearance for the standalone merge and diff processes.
//! This module does not read repository content or change tool actions.

use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, Vec2};

use crate::theme::{self, FontSelection, TextContrast, ThemeAccent, ThemeMode};

pub(crate) struct Appearance {
    pub accent: ThemeAccent,
    pub contrast: TextContrast,
    pub fonts: FontSelection,
    pub scale: f32,
}

pub(crate) fn install(ctx: &egui::Context, mode: ThemeMode) {
    let appearance = crate::app::load_tool_appearance();
    theme::install(ctx);
    theme::set_text_contrast(ctx, appearance.contrast);
    theme::apply(ctx, mode, appearance.accent);
    ctx.set_zoom_factor(appearance.scale);
    if appearance.fonts != FontSelection::default() {
        let catalog = theme::scan_system_fonts();
        if let Ok(fonts) = theme::load_font_set(&appearance.fonts, &catalog) {
            theme::apply_loaded_font_set(ctx, fonts);
        }
    }
    remember_theme(ctx, mode);
}

fn remember_theme(ctx: &egui::Context, mode: ThemeMode) {
    let selection = (
        mode,
        theme::current_accent(),
        theme::current_text_contrast(),
    );
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("standalone_tool_theme"), selection));
}

pub(crate) fn apply_theme(ctx: &egui::Context, mode: ThemeMode) -> bool {
    let selection = (
        mode,
        theme::current_accent(),
        theme::current_text_contrast(),
    );
    let unchanged = ctx.data(|data| {
        data.get_temp::<(ThemeMode, ThemeAccent, TextContrast)>(egui::Id::new(
            "standalone_tool_theme",
        )) == Some(selection)
    });
    if unchanged {
        return false;
    }
    theme::apply(ctx, mode, selection.1);
    remember_theme(ctx, mode);
    true
}

pub(crate) fn surface_shadow(palette: theme::Palette) -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: [3, 4],
        blur: 9,
        spread: 0,
        color: palette.accent_shadow,
    }
}

pub(crate) fn paint_code_panel(ui: &egui::Ui, rect: egui::Rect) {
    let palette = theme::palette(theme::current_mode());
    let frame = egui::Frame::new()
        .fill(palette.panel)
        .corner_radius(egui::CornerRadius::same(8))
        .shadow(surface_shadow(palette));
    ui.painter().add(frame.paint(rect));
}

pub(crate) fn paint_scroll_viewport_inset_shadow(ui: &egui::Ui, rect: egui::Rect) {
    if !rect.is_positive() || !rect.is_finite() {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let shadow = theme::inset_shadow();
    let highlight = theme::inset_highlight();
    let depth = (rect.width().min(rect.height()) * 0.5).min(6.0).floor() as usize;
    for layer in 0..depth {
        let edge = rect.shrink(layer as f32);
        let fade = (1.0 - layer as f32 / depth as f32).powi(2);
        let alpha = (f32::from(shadow.a().max(128)) * 0.46 * fade).round() as u8;
        let dark = Color32::from_rgba_unmultiplied(shadow.r(), shadow.g(), shadow.b(), alpha);
        painter.rect_filled(
            egui::Rect::from_min_max(edge.left_top(), egui::pos2(edge.right(), edge.top() + 1.0)),
            0.0,
            dark,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(edge.left(), edge.top() + 1.0),
                egui::pos2(edge.left() + 1.0, edge.bottom()),
            ),
            0.0,
            dark,
        );
        if layer < 2 {
            let light = Color32::from_rgba_unmultiplied(
                highlight.r(),
                highlight.g(),
                highlight.b(),
                (f32::from(highlight.a()) * 0.35 * fade).round() as u8,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(edge.left() + 1.0, edge.bottom() - 1.0),
                    edge.right_bottom(),
                ),
                0.0,
                light,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(edge.right() - 1.0, edge.top() + 1.0),
                    edge.right_bottom(),
                ),
                0.0,
                light,
            );
        }
    }
}

pub(crate) fn icon_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    tooltip: &str,
    active: bool,
) -> egui::Response {
    let image = egui::Image::new(icon)
        .fit_to_exact_size(Vec2::splat(15.0))
        .tint(if active {
            Color32::WHITE
        } else {
            theme::text()
        });
    ui.add(
        egui::Button::image(image)
            .min_size(Vec2::splat(28.0))
            .fill(if active {
                theme::accent_deep()
            } else {
                theme::panel_soft()
            })
            .stroke(Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(4)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text(tooltip)
}

pub(crate) fn window_control_button(ui: &mut egui::Ui, label: &str, close: bool) -> egui::Response {
    // Keep the tools' existing hit rectangles and drag exclusion boundaries.
    let (rect, response) = ui.allocate_exact_size(Vec2::new(34.0, 22.0), Sense::click());
    let fill = if close {
        Color32::from_rgb(192, 55, 43)
    } else if response.hovered() {
        theme::hover()
    } else {
        theme::panel_soft()
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(4), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        if close { Color32::WHITE } else { theme::text() },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_tool_theme_keeps_context_style_and_dense_layout() {
        let ctx = egui::Context::default();
        assert!(apply_theme(&ctx, ThemeMode::Light));
        ctx.style_mut(|style| style.spacing.item_spacing = egui::vec2(3.0, 2.0));
        assert!(!apply_theme(&ctx, ThemeMode::Light));
        assert_eq!(ctx.style().spacing.item_spacing, egui::vec2(3.0, 2.0));
        assert!(apply_theme(&ctx, ThemeMode::Dark));
        assert!(ctx.style().visuals.dark_mode);
    }
}
