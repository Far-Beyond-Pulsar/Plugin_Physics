//! Right dock panel of the material editor: the material's numbers, how they
//! combine, its surface type, and what the validator thinks of it.

use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::input::{InputEvent, InputState, TextInput};
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, IconName, Selectable, Sizable, StyledExt};

use crate::assets::material::{
    matching_preset, CombineRule, PhysicsMaterial, SurfaceType, WATER_DENSITY,
};
use crate::assets::Severity;
use crate::editors::common::chrome::{
    all_clear, color_of, issue_row, panel_header, section, swatch, MONO,
};
use crate::editors::common::document::Document;
use crate::editors::common::fields::{ScalarField, ScalarSpec};
use crate::editors::common::panel::dock_panel;

type Doc = Entity<Document<PhysicsMaterial>>;

pub struct MaterialProperties {
    doc: Doc,
    friction: ScalarField<PhysicsMaterial>,
    restitution: ScalarField<PhysicsMaterial>,
    density: ScalarField<PhysicsMaterial>,
    description: Entity<InputState>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

dock_panel!(
    MaterialProperties,
    "physics-material-properties",
    "Properties"
);

impl MaterialProperties {
    pub fn new(doc: Doc, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let friction = ScalarField::new(
            &doc,
            ScalarSpec {
                label: "Change friction",
                key: "friction",
                min: 0.0,
                max: 2.0,
                hard_max: 20.0,
                step: 0.01,
                decimals: 2,
                get: |m: &PhysicsMaterial| m.friction,
                set: |m, v| m.friction = v,
            },
            window,
            cx,
        );
        let restitution = ScalarField::new(
            &doc,
            ScalarSpec {
                label: "Change restitution",
                key: "restitution",
                min: 0.0,
                max: 1.0,
                hard_max: 2.0,
                step: 0.01,
                decimals: 2,
                get: |m: &PhysicsMaterial| m.restitution,
                set: |m, v| m.restitution = v,
            },
            window,
            cx,
        );
        let density = ScalarField::new(
            &doc,
            ScalarSpec {
                label: "Change density",
                key: "density",
                min: 0.0,
                max: 8000.0,
                hard_max: 30_000.0,
                step: 10.0,
                decimals: 1,
                get: |m: &PhysicsMaterial| m.density,
                set: |m, v| m.density = v,
            },
            window,
            cx,
        );

        let initial = doc.read(cx).asset().description.clone();
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("What is this material for?")
                .default_value(initial)
        });
        let d = doc.clone();
        let subscription = cx.subscribe_in(
            &description,
            window,
            move |_this, input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    if d.read(cx).asset().description != text {
                        d.update(cx, |d, cx| {
                            d.edit("Edit description", Some("description"), cx, |m| {
                                m.description = text
                            })
                        });
                    }
                }
            },
        );

        Self {
            doc,
            friction,
            restitution,
            density,
            description,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    /// A row of buttons choosing one combine rule, with the chosen rule explained.
    fn combine_picker(
        &self,
        id: &'static str,
        current: CombineRule,
        label: &'static str,
        set: fn(&mut PhysicsMaterial, CombineRule),
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let buttons = CombineRule::ALL.map(|rule| {
            let doc = self.doc.clone();
            Button::new(SharedString::from(format!("{id}-{}", rule.id())))
                .label(rule.label())
                .ghost()
                .xsmall()
                .selected(rule == current)
                .tooltip(rule.description())
                .on_click(move |_, _, cx| {
                    doc.update(cx, |d, cx| d.edit(label, None, cx, |m| set(m, rule)));
                })
        });
        v_flex()
            .gap_1()
            .child(h_flex().gap_0p5().flex_wrap().children(buttons))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(current.description()),
            )
    }

    fn render_identity(&self, m: &PhysicsMaterial, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let doc = self.doc.read(cx);
        let preset = matching_preset(m);

        let chips = SurfaceType::ALL.map(|surface| {
            let doc = self.doc.clone();
            h_flex().gap_1().items_center().child(
                Button::new(SharedString::from(format!("surface-{}", surface.id())))
                    .label(surface.label())
                    .ghost()
                    .xsmall()
                    .selected(surface == m.surface)
                    .on_click(move |_, _, cx| {
                        doc.update(cx, |d, cx| {
                            d.edit("Change surface type", None, cx, |m| m.surface = surface)
                        });
                    }),
            )
        });

        v_flex()
            .w_full()
            .p_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(swatch(color_of(m.surface.tint()), 26.0, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_base()
                                    .font_semibold()
                                    .text_color(theme.foreground)
                                    .truncate()
                                    .child(doc.file_name()),
                            )
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                preset.map_or_else(
                                    || format!("{} surface", m.surface.label()),
                                    |p| format!("Matches the {} preset", p.name),
                                ),
                            )),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Surface type"),
                    )
                    .child(h_flex().gap_0p5().flex_wrap().children(chips)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Notes"),
                    )
                    .child(TextInput::new(&self.description).small()),
            )
    }

    fn render_density_hint(&self, m: &PhysicsMaterial, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let relative = if m.density > 0.0 {
            m.density / WATER_DENSITY
        } else {
            0.0
        };
        h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(
                div()
                    .text_xs()
                    .font_family(MONO)
                    .text_color(theme.foreground.opacity(0.9))
                    .child(format!("1 m³ weighs {:.1} kg", m.density)),
            )
            .child(if m.floats() {
                Tag::info().small().child("Floats").into_any_element()
            } else {
                Tag::secondary().small().child("Sinks").into_any_element()
            })
            .child(
                div()
                    .text_xs()
                    .font_family(MONO)
                    .text_color(theme.muted_foreground)
                    .child(format!("{relative:.2}× water")),
            )
    }

    fn render_issues(&self, m: &PhysicsMaterial, cx: &App) -> impl IntoElement {
        let issues = m.validate();
        let doc = self.doc.clone();
        let rows: Vec<AnyElement> = issues
            .iter()
            .enumerate()
            .map(|(i, issue)| {
                let doc = doc.clone();
                let fixes = issue
                    .fix
                    .map(|f| vec![(f.label().to_string(), f)])
                    .unwrap_or_default();
                issue_row(
                    SharedString::from(format!("material-fix-{i}")),
                    issue,
                    fixes,
                    move |fix, _, cx| {
                        doc.update(cx, |d, cx| {
                            d.edit("Fix material", None, cx, |m| fix.apply(m))
                        });
                    },
                    cx,
                )
                .into_any_element()
            })
            .collect();

        if rows.is_empty() {
            all_clear("Nothing wrong with this material", cx).into_any_element()
        } else {
            v_flex().gap_1p5().children(rows).into_any_element()
        }
    }
}

impl Render for MaterialProperties {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let m = self.doc.read(cx).asset().clone();

        // Controls follow the document: undo, redo and presets move them.
        self.friction.sync(m.friction, window, cx);
        self.restitution.sync(m.restitution, window, cx);
        self.density.sync(m.density, window, cx);
        let focused = self
            .description
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx);
        if !focused && self.description.read(cx).value().as_ref() != m.description.as_str() {
            let text = m.description.clone();
            self.description
                .update(cx, |i, cx| i.set_value(text, window, cx));
        }

        let issues = m.validate();
        let worst = issues.iter().map(|i| i.severity).max();
        let badge = match worst {
            Some(Severity::Error) => Some(
                Tag::danger()
                    .small()
                    .child(format!("{} issues", issues.len())),
            ),
            Some(Severity::Warning) => Some(
                Tag::warning()
                    .small()
                    .child(format!("{} issues", issues.len())),
            ),
            Some(Severity::Info) => {
                Some(Tag::info().small().child(format!("{} notes", issues.len())))
            }
            None => None,
        };

        let friction_section = section(
            "Friction",
            Some(
                "How strongly surfaces grip each other. 0 is ice, around 1 is rubber on asphalt."
                    .into(),
            ),
            v_flex()
                .gap_2()
                .child(self.friction.render(false, cx))
                .child(self.combine_picker(
                    "friction-combine",
                    m.friction_combine,
                    "Change friction combine mode",
                    |m, r| m.friction_combine = r,
                    cx,
                )),
            cx,
        );
        let restitution_section = section(
            "Restitution",
            Some("Bounciness: the share of speed kept after an impact. 0 is a dead thud, 1 never loses energy.".into()),
            v_flex()
                .gap_2()
                .child(self.restitution.render(false, cx))
                .child(self.combine_picker(
                    "restitution-combine",
                    m.restitution_combine,
                    "Change restitution combine mode",
                    |m, r| m.restitution_combine = r,
                    cx,
                )),
            cx,
        );
        let density_section = section(
            "Density",
            Some("kg per cubic metre. A body with mass 0 derives its mass from its colliders' volume times this.".into()),
            v_flex()
                .gap_2()
                .child(self.density.render(false, cx))
                .child(self.render_density_hint(&m, cx)),
            cx,
        );
        let issues_section = section("Validation", None, self.render_issues(&m, cx), cx);

        v_flex()
            .id("material-properties")
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(
                IconName::Settings,
                "Material",
                badge.map(|b| b.into_any_element()),
                cx,
            ))
            .child(
                v_flex()
                    .id("material-properties-scroll")
                    .flex_1()
                    .min_h_0()
                    .scrollable(ScrollbarAxis::Vertical)
                    .child(self.render_identity(&m, cx))
                    .child(friction_section)
                    .child(restitution_section)
                    .child(density_section)
                    .child(issues_section),
            )
    }
}
