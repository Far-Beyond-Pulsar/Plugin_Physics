//! Left dock panel of the material editor: real-world starting points.

use gpui::prelude::FluentBuilder;
use gpui::*;
use ui::input::{InputEvent, InputState, TextInput};
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, Icon, IconName, Sizable, StyledExt};

use crate::assets::material::{matching_preset, PhysicsMaterial, Preset, PRESETS, PRESET_CATEGORIES};
use crate::editors::common::chrome::{color_of, empty_message, panel_header, swatch, MONO};
use crate::editors::common::document::Document;
use crate::editors::common::panel::dock_panel;

type Doc = Entity<Document<PhysicsMaterial>>;

pub struct PresetsPanel {
    doc: Doc,
    search: Entity<InputState>,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

dock_panel!(PresetsPanel, "physics-material-presets", "Presets");

/// Apply `preset` to `material`: its numbers and surface, keeping the combine
/// modes and any fields the preset knows nothing about. Notes are replaced
/// only when empty or when they are another preset's own.
pub fn apply_preset(material: &mut PhysicsMaterial, preset: &Preset) {
    material.friction = preset.friction;
    material.restitution = preset.restitution;
    material.density = preset.density;
    material.surface = preset.surface;
    let notes_are_ours = material.description.is_empty() || PRESETS.iter().any(|p| p.note == material.description);
    if notes_are_ours {
        material.description = preset.note.to_string();
    }
}

impl PresetsPanel {
    pub fn new(doc: Doc, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search presets…"));
        let subscription = cx.subscribe_in(&search, window, |_this, _input, event: &InputEvent, _w, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self { doc, search, focus_handle: cx.focus_handle(), _subscription: subscription }
    }

    fn row(&self, preset: &'static Preset, active: bool, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let doc = self.doc.clone();
        h_flex()
            .id(SharedString::from(format!("preset-{}", preset.name)))
            .relative()
            .w_full()
            .h(px(40.0))
            .px_2()
            .gap_2()
            .items_center()
            .cursor_pointer()
            .bg(if active { theme.list_active } else { theme.transparent })
            .hover(|s| s.bg(theme.list_hover))
            .tooltip({
                let note = preset.note;
                move |window, cx| ui::tooltip::Tooltip::new(note).build(window, cx)
            })
            .when(active, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.0))
                        .bg(theme.primary),
                )
            })
            .on_click(move |_, _, cx| {
                doc.update(cx, |d, cx| {
                    d.edit("Apply preset", None, cx, |m| apply_preset(m, preset));
                });
            })
            .child(swatch(color_of(preset.surface.tint()), 22.0, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                            .text_color(theme.foreground)
                            .truncate()
                            .child(preset.name),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_family(MONO)
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "μ {:.2}  e {:.2}  ρ {:.0}",
                                preset.friction, preset.restitution, preset.density
                            )),
                    ),
            )
            .when(active, |row| {
                row.child(Icon::new(IconName::Check).size(px(13.0)).text_color(theme.primary))
            })
    }
}

impl Render for PresetsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let current = matching_preset(self.doc.read(cx).asset());

        let mut list = v_flex().w_full().pb_2();
        let mut shown = 0usize;
        for category in PRESET_CATEGORIES {
            let presets: Vec<&'static Preset> = PRESETS
                .iter()
                .filter(|p| p.category == category)
                .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query) || p.surface.id().contains(&query))
                .collect();
            if presets.is_empty() {
                continue;
            }
            shown += presets.len();
            list = list.child(
                h_flex()
                    .w_full()
                    .px_3()
                    .pt_2p5()
                    .pb_1()
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_semibold()
                            .text_color(theme.muted_foreground)
                            .child(category.to_uppercase()),
                    ),
            );
            for preset in presets {
                let active = current.is_some_and(|c| std::ptr::eq(c, preset));
                list = list.child(self.row(preset, active, cx));
            }
        }

        let body = if shown == 0 {
            div()
                .flex_1()
                .min_h_0()
                .child(empty_message(IconName::Search, "No presets match", "Try a surface name such as wood or metal", cx))
                .into_any_element()
        } else {
            v_flex()
                .id("presets-scroll")
                .flex_1()
                .min_h_0()
                .scrollable(ScrollbarAxis::Vertical)
                .child(list)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(
                IconName::Droplet,
                "Presets",
                Some(Tag::secondary().small().rounded_full().child(PRESETS.len().to_string()).into_any_element()),
                cx,
            ))
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .px_2()
                    .py_1p5()
                    .border_b_1()
                    .border_color(theme.border.opacity(0.6))
                    .child(
                        TextInput::new(&self.search).small().cleanable().prefix(
                            Icon::new(IconName::Search).size_3p5().text_color(theme.muted_foreground),
                        ),
                    ),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::apply_preset;
    use crate::assets::material::{CombineRule, PhysicsMaterial, PRESETS};

    #[test]
    fn applying_a_preset_keeps_combine_modes_and_custom_notes() {
        let ice = PRESETS.iter().find(|p| p.name == "Ice").unwrap();
        let mut m = PhysicsMaterial {
            friction_combine: CombineRule::Min,
            description: "Used by the skating rink".into(),
            ..Default::default()
        };
        m.extra.insert("future".into(), serde_json::json!(1));
        apply_preset(&mut m, ice);
        assert_eq!(m.friction, 0.03);
        assert_eq!(m.friction_combine, CombineRule::Min);
        assert_eq!(m.description, "Used by the skating rink");
        assert!(m.extra.contains_key("future"));

        // Notes that came from another preset are replaced.
        let rubber = PRESETS.iter().find(|p| p.name == "Rubber").unwrap();
        let mut from_preset = ice.to_material();
        apply_preset(&mut from_preset, rubber);
        assert_eq!(from_preset.description, rubber.note);
    }
}
