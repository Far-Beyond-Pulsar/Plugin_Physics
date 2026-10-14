//! Bottom dock of the material editor: how this material behaves against
//! every preset, and what bodies made of it weigh.

use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::input::{InputEvent, InputState, NumberInput};
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, IconName, Selectable, Sizable, StyledExt};

use crate::assets::material::{CombineRule, PhysicsMaterial, Preset, PRESETS, WATER_DENSITY};
use crate::editors::common::chrome::{caption, color_of, panel_header, swatch, MONO};
use crate::editors::common::document::Document;
use crate::editors::common::panel::dock_panel;

type Doc = Entity<Document<PhysicsMaterial>>;

// ── Interaction matrix ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Name,
    Friction,
    Restitution,
}

/// One row of the table: a preset and what the contact with it comes to.
struct Row {
    preset: &'static Preset,
    friction: f32,
    restitution: f32,
    friction_rule: CombineRule,
    restitution_rule: CombineRule,
}

fn rows_for(material: &PhysicsMaterial, sort: SortBy, descending: bool) -> Vec<Row> {
    let mut rows: Vec<Row> = PRESETS
        .iter()
        .map(|preset| {
            let hit = material.contact_with(&preset.to_material());
            Row {
                preset,
                friction: hit.friction,
                restitution: hit.restitution,
                friction_rule: hit.friction_rule,
                restitution_rule: hit.restitution_rule,
            }
        })
        .collect();
    rows.sort_by(|a, b| match sort {
        SortBy::Name => a.preset.name.cmp(b.preset.name),
        SortBy::Friction => a.friction.total_cmp(&b.friction),
        SortBy::Restitution => a.restitution.total_cmp(&b.restitution),
    });
    if descending {
        rows.reverse();
    }
    rows
}

pub struct InteractionPanel {
    doc: Doc,
    sort: SortBy,
    descending: bool,
    focus_handle: FocusHandle,
}

dock_panel!(
    InteractionPanel,
    "physics-material-interactions",
    "Interactions"
);

impl InteractionPanel {
    pub fn new(doc: Doc, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            doc,
            sort: SortBy::Name,
            descending: false,
            focus_handle: cx.focus_handle(),
        }
    }

    fn sort_button(
        &self,
        id: &'static str,
        label: &'static str,
        by: SortBy,
        cx: &mut Context<Self>,
    ) -> Button {
        let active = self.sort == by;
        let arrow = match (active, self.descending) {
            (true, true) => " ↓",
            (true, false) => " ↑",
            _ => "",
        };
        Button::new(id)
            .label(format!("{label}{arrow}"))
            .ghost()
            .xsmall()
            .selected(active)
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.sort == by {
                    this.descending = !this.descending;
                } else {
                    this.sort = by;
                    this.descending = false;
                }
                cx.notify();
            }))
    }
}

/// A horizontal bar `fraction` full, with its value on the right.
fn meter(
    fraction: f32,
    color: Hsla,
    value: String,
    rule: Option<CombineRule>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .flex_1()
        .min_w(px(120.0))
        .gap_2()
        .items_center()
        .child(
            div()
                .flex_1()
                .h(px(8.0))
                .rounded(px(4.0))
                .bg(theme.foreground.opacity(0.07))
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction.clamp(0.0, 1.0)))
                        .rounded(px(4.0))
                        .bg(color),
                ),
        )
        .child(
            div()
                .w(px(40.0))
                .text_xs()
                .font_family(MONO)
                .text_color(theme.foreground.opacity(0.9))
                .child(value),
        )
        .children(
            rule.filter(|r| *r != CombineRule::Average)
                .map(|r| Tag::secondary().small().child(r.label())),
        )
}

impl Render for InteractionPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let material = self.doc.read(cx).asset().clone();
        let rows = rows_for(&material, self.sort, self.descending);

        let header = h_flex()
            .w_full()
            .flex_shrink_0()
            .px_3()
            .py_1()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .child(div().w(px(150.0)).child(self.sort_button(
                "sort-name",
                "Surface",
                SortBy::Name,
                cx,
            )))
            .child(div().flex_1().min_w(px(120.0)).child(self.sort_button(
                "sort-friction",
                "Friction",
                SortBy::Friction,
                cx,
            )))
            .child(div().flex_1().min_w(px(120.0)).child(self.sort_button(
                "sort-restitution",
                "Restitution",
                SortBy::Restitution,
                cx,
            )));

        let list = v_flex()
            .w_full()
            .py_1()
            .children(rows.into_iter().map(|row| {
                h_flex()
                    .w_full()
                    .px_3()
                    .h(px(28.0))
                    .gap_2()
                    .items_center()
                    .hover(|s| s.bg(theme.list_hover))
                    .child(
                        h_flex()
                            .w(px(150.0))
                            .gap_2()
                            .items_center()
                            .child(swatch(color_of(row.preset.surface.tint()), 14.0, cx))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.foreground)
                                    .truncate()
                                    .child(row.preset.name),
                            ),
                    )
                    .child(meter(
                        row.friction / 2.0,
                        theme.warning,
                        format!("{:.2}", row.friction),
                        Some(row.friction_rule),
                        cx,
                    ))
                    .child(meter(
                        row.restitution,
                        theme.info,
                        format!("{:.2}", row.restitution),
                        Some(row.restitution_rule),
                        cx,
                    ))
            }));

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(
                IconName::Table,
                "Interactions",
                Some(caption("this material against each preset", cx).into_any_element()),
                cx,
            ))
            .child(header)
            .child(
                v_flex()
                    .id("interactions-scroll")
                    .flex_1()
                    .min_h_0()
                    .scrollable(ScrollbarAxis::Vertical)
                    .child(list),
            )
    }
}

// ── Mass calculator ────────────────────────────────────────────────────────

pub struct MassPanel {
    doc: Doc,
    size: [Entity<InputState>; 3],
    radius: Entity<InputState>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

dock_panel!(MassPanel, "physics-material-mass", "Mass");

impl MassPanel {
    pub fn new(doc: Doc, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let make = |value: &str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()))
        };
        let size = [
            make("1", window, cx),
            make("1", window, cx),
            make("1", window, cx),
        ];
        let radius = make("0.5", window, cx);
        let mut subscriptions = Vec::new();
        for input in size.iter().chain(std::iter::once(&radius)) {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |_this, _i, event: &InputEvent, _w, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ));
        }
        Self {
            doc,
            size,
            radius,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn value(input: &Entity<InputState>, cx: &App, default: f32) -> f32 {
        input
            .read(cx)
            .value()
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(default)
    }
}

impl Render for MassPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let material = self.doc.read(cx).asset().clone();
        let dims = [
            Self::value(&self.size[0], cx, 1.0),
            Self::value(&self.size[1], cx, 1.0),
            Self::value(&self.size[2], cx, 1.0),
        ];
        let radius = Self::value(&self.radius, cx, 0.5);
        let volume = dims[0] * dims[1] * dims[2];
        let box_mass = material.box_mass(dims);
        let sphere_mass = material.sphere_mass(radius);
        let water = WATER_DENSITY * volume;

        let mono = |text: String| {
            div()
                .text_sm()
                .font_family(MONO)
                .text_color(theme.foreground)
                .child(text)
        };
        let dim_input = |input: &Entity<InputState>, label: &'static str| {
            v_flex()
                .gap_0p5()
                .w(px(86.0))
                .child(caption(label, cx))
                .child(NumberInput::new(input).small())
        };

        // The same volume in a few reference materials, for scale.
        let reference: Vec<(String, f32, bool)> =
            std::iter::once(("This material".to_string(), box_mass, true))
                .chain(
                    ["Wood", "Concrete", "Aluminium", "Steel"]
                        .iter()
                        .filter_map(|name| {
                            PRESETS
                                .iter()
                                .find(|p| p.name == *name)
                                .map(|p| (p.name.to_string(), p.density * volume, false))
                        }),
                )
                .collect();
        let heaviest = reference
            .iter()
            .map(|(_, m, _)| *m)
            .fold(f32::MIN_POSITIVE, f32::max);

        let bars = v_flex()
            .gap_1()
            .children(reference.into_iter().map(|(name, mass, this)| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(96.0))
                            .text_xs()
                            .text_color(if this {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(8.0))
                            .rounded(px(4.0))
                            .bg(theme.foreground.opacity(0.07))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative((mass / heaviest).clamp(0.0, 1.0)))
                                    .rounded(px(4.0))
                                    .bg(if this {
                                        theme.primary
                                    } else {
                                        theme.muted_foreground.opacity(0.6)
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(78.0))
                            .text_xs()
                            .font_family(MONO)
                            .text_color(theme.foreground.opacity(0.9))
                            .child(format!("{mass:.1} kg")),
                    )
            }));

        let body =
            h_flex()
                .w_full()
                .p_3()
                .gap_4()
                .flex_wrap()
                .items_start()
                .child(
                    v_flex()
                        .gap_2()
                        .child(caption("Box (metres)", cx))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(dim_input(&self.size[0], "X"))
                                .child(dim_input(&self.size[1], "Y"))
                                .child(dim_input(&self.size[2], "Z")),
                        )
                        .child(mono(format!("{volume:.3} m³  →  {box_mass:.2} kg")))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(dim_input(&self.radius, "Sphere radius"))
                                .child(mono(format!("{sphere_mass:.2} kg"))),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(260.0))
                        .gap_2()
                        .child(caption("Same box, other materials", cx))
                        .child(bars)
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(if material.floats() {
                                    Tag::info().small().child("Floats").into_any_element()
                                } else {
                                    Tag::secondary().small().child("Sinks").into_any_element()
                                })
                                .child(div().text_xs().text_color(theme.muted_foreground).child(
                                    format!("the same volume of water weighs {water:.1} kg"),
                                )),
                        ),
                );

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(IconName::Weight, "Mass", None, cx))
            .child(
                v_flex()
                    .id("mass-scroll")
                    .flex_1()
                    .min_h_0()
                    .scrollable(ScrollbarAxis::Vertical)
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{rows_for, SortBy};
    use crate::assets::material::PhysicsMaterial;

    #[test]
    fn rows_cover_every_preset_and_sort() {
        let m = PhysicsMaterial::default();
        let by_friction = rows_for(&m, SortBy::Friction, false);
        assert_eq!(by_friction.len(), crate::assets::material::PRESETS.len());
        assert!(by_friction
            .windows(2)
            .all(|w| w[0].friction <= w[1].friction));
        let descending = rows_for(&m, SortBy::Restitution, true);
        assert!(descending
            .windows(2)
            .all(|w| w[0].restitution >= w[1].restitution));
        let names = rows_for(&m, SortBy::Name, false);
        assert!(names
            .windows(2)
            .all(|w| w[0].preset.name <= w[1].preset.name));
    }
}
