//! Ready-made layer setups for common kinds of game.

use gpui::*;
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, IconName, Sizable, StyledExt};

use crate::assets::layers::{LayerPreset, PRESETS};
use crate::editors::common::chrome::{color_of, panel_header, swatch};
use crate::editors::common::panel::dock_panel;

use super::state::{Doc, Sel};

pub struct LayerPresets {
    doc: Doc,
    sel: Sel,
    focus_handle: FocusHandle,
}

dock_panel!(LayerPresets, "physics-layers-presets", "Presets");

/// Replace every layer with `preset`'s, keeping fields this version doesn't know.
pub fn apply(set: &mut crate::assets::layers::CollisionLayers, preset: &LayerPreset) {
    let extra = std::mem::take(&mut set.extra);
    let version = set.version;
    *set = preset.build();
    set.extra = extra;
    set.version = version;
}

impl LayerPresets {
    pub fn new(doc: Doc, sel: Sel, cx: &mut Context<Self>) -> Self {
        Self { doc, sel, focus_handle: cx.focus_handle() }
    }
}

impl Render for LayerPresets {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows = PRESETS.iter().enumerate().map(|(n, preset)| {
            let built = preset.build();
            let (doc, sel) = (self.doc.clone(), self.sel.clone());
            let strip = (0..built.len().min(12)).map(|i| swatch(color_of(built.color_of(i)), 11.0, cx));
            v_flex()
                .id(SharedString::from(format!("layer-preset-{n}")))
                .w_full()
                .px_3()
                .py_2()
                .gap_1()
                .cursor_pointer()
                .border_b_1()
                .border_color(theme.border.opacity(0.5))
                .hover(|s| s.bg(theme.list_hover))
                .on_click(move |_, _, cx| {
                    doc.update(cx, |d, cx| d.edit("Apply layer preset", None, cx, |set| apply(set, preset)));
                    sel.update(cx, |s, cx| s.select(None, cx));
                })
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1().text_sm().font_semibold().text_color(theme.foreground).child(preset.name))
                        .child(Tag::secondary().small().child(format!("{} layers", built.len()))),
                )
                .child(div().text_xs().text_color(theme.muted_foreground).child(preset.description))
                .child(h_flex().gap_0p5().flex_wrap().children(strip))
        });

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(IconName::Network, "Presets", None, cx))
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .px_3()
                    .py_1p5()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Replaces all layers. Undo (Ctrl+Z) brings yours back."),
            )
            .child(v_flex().id("layer-presets-scroll").flex_1().min_h_0().scrollable(ScrollbarAxis::Vertical).children(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::assets::layers::{CollisionLayers, PRESETS};

    #[test]
    fn applying_a_preset_keeps_unknown_fields() {
        let mut set = CollisionLayers::default();
        set.extra.insert("future".into(), serde_json::json!(true));
        set.version = 3;
        apply(&mut set, &PRESETS[1]);
        assert_eq!(set.len(), PRESETS[1].rows.len());
        assert!(set.extra.contains_key("future"));
        assert_eq!(set.version, 3);
    }
}
