//! Center panel of the material editor: what the numbers *do*.
//!
//! A ball dropped onto a surface, and a block on an incline. Both are
//! computed in closed form from the material combined with the chosen
//! surface, so they follow the sliders in real time.

use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::scroll::ScrollbarAxis;
use ui::slider::{Slider, SliderEvent, SliderState};
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, IconName, Selectable, Sizable, StyledExt};

use crate::assets::material::{
    bounce_arcs, settle_time, slide_distance, slope_state, PhysicsMaterial, GRAVITY, PRESETS,
};
use crate::editors::common::chrome::{caption, panel_header, MONO};
use crate::editors::common::document::Document;
use crate::editors::common::panel::dock_panel;

use super::charts::{BouncePlot, SlopePlot, PLOT_BOTTOM, PLOT_LEFT, PLOT_RIGHT, PLOT_TOP};
use crate::editors::common::paint::ChartColors;

type Doc = Entity<Document<PhysicsMaterial>>;

/// What the previewed material lands on / rests against.
const AGAINST: [&str; 6] = ["Itself", "Concrete", "Steel", "Wood", "Rubber", "Ice"];

/// The other half of the contact: the material itself, or a named preset.
pub fn partner(own: &PhysicsMaterial, against: usize) -> PhysicsMaterial {
    match AGAINST.get(against) {
        Some(name) if *name != "Itself" => PRESETS
            .iter()
            .find(|p| p.name == *name)
            .map_or_else(|| own.clone(), |p| p.to_material()),
        _ => own.clone(),
    }
}

pub struct PreviewPanel {
    doc: Doc,
    drop_height: Entity<SliderState>,
    angle: Entity<SliderState>,
    bounce_against: usize,
    slope_against: usize,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

dock_panel!(PreviewPanel, "physics-material-preview", "Preview");

impl PreviewPanel {
    pub fn new(doc: Doc, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let drop_height = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(5.0)
                .step(0.05)
                .default_value(2.0)
        });
        let angle = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(70.0)
                .step(0.5)
                .default_value(25.0)
        });
        let redraw = |_this: &mut Self,
                      _s: Entity<SliderState>,
                      _e: &SliderEvent,
                      cx: &mut Context<Self>| cx.notify();
        let subscriptions = vec![
            cx.subscribe(&drop_height, redraw),
            cx.subscribe(&angle, redraw),
        ];
        Self {
            doc,
            drop_height,
            angle,
            bounce_against: 0,
            slope_against: 0,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn colors(cx: &App) -> ChartColors {
        let t = cx.theme();
        ChartColors {
            grid: t.border.opacity(0.55),
            axis: t.muted_foreground,
            line: t.info,
            fill: t.info.opacity(0.13),
            accent: t.warning,
            text: t.foreground,
            muted: t.muted_foreground,
            good: t.success,
            bad: t.danger,
            info: t.info,
            warn: t.warning,
        }
    }

    fn against_picker(
        &self,
        id: &'static str,
        current: usize,
        set: fn(&mut Self, usize),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let buttons = AGAINST.iter().enumerate().map(|(i, name)| {
            Button::new(SharedString::from(format!("{id}-{name}")))
                .label(*name)
                .ghost()
                .xsmall()
                .selected(i == current)
                .on_click(cx.listener(move |this, _, _, cx| {
                    set(this, i);
                    cx.notify();
                }))
        });
        h_flex().gap_0p5().flex_wrap().children(buttons)
    }

    fn card(title: &'static str, subtitle: &'static str, cx: &App) -> Div {
        let t = cx.theme();
        v_flex()
            .flex_1()
            .min_w(px(330.0))
            .gap_2()
            .p_3()
            .rounded(px(10.0))
            .bg(t.background)
            .border_1()
            .border_color(t.border.opacity(0.7))
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(t.foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.muted_foreground)
                            .child(subtitle),
                    ),
            )
    }

    fn render_bounce(&self, own: &PhysicsMaterial, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let other = partner(own, self.bounce_against);
        let contact = own.contact_with(&other);
        let height = self.drop_height.read(cx).value().end().max(0.05);
        let e = contact.restitution;

        let arcs = bounce_arcs(height, e, GRAVITY, 0.004, 48);
        let bounces = arcs.iter().filter(|a| !a.drop).count();
        let settle = settle_time(height, e, GRAVITY);
        let first = arcs.get(1).map(|a| a.peak);
        let path_end = arcs.last().map_or(1.0, |a| a.end);
        let duration = match settle {
            Some(total) => total.min(path_end * 1.04 + 0.1).max(path_end),
            None => path_end,
        }
        .max(0.5);

        let plot = BouncePlot {
            arcs,
            gravity: GRAVITY,
            drop_height: height,
            duration,
            colors: Self::colors(cx),
        };

        let mono = |text: String, theme: &ui::Theme| {
            div()
                .text_xs()
                .font_family(MONO)
                .text_color(theme.foreground.opacity(0.9))
                .child(text)
        };

        Self::card("Bounce", "A ball dropped onto the surface", cx)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(caption("Against", cx))
                    .child(self.against_picker(
                        "bounce-against",
                        self.bounce_against,
                        |this, i| this.bounce_against = i,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(caption("Drop height", cx))
                    .child(div().flex_1().child(Slider::new(&self.drop_height)))
                    .child(
                        div()
                            .w(px(52.0))
                            .child(mono(format!("{height:.2} m"), &theme)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(
                        Tag::info()
                            .small()
                            .child(format!("e = {e:.2} ({})", contact.restitution_rule.label())),
                    )
                    .child(mono(
                        match bounces {
                            0 => "does not bounce".to_string(),
                            n if n >= 48 => "48+ bounces".to_string(),
                            n => format!("{n} bounce{}", if n == 1 { "" } else { "s" }),
                        },
                        &theme,
                    ))
                    .children(first.map(|peak| mono(format!("first rebound {peak:.2} m"), &theme)))
                    .child(mono(
                        settle.map_or("never settles".to_string(), |t| {
                            format!("rests after {t:.1} s")
                        }),
                        &theme,
                    )),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(190.0))
                    .child(
                        gpui::canvas(
                            |bounds, _window, _cx| bounds,
                            move |_bounds, bounds, window, _cx| plot.paint(bounds, window),
                        )
                        .size_full(),
                    )
                    .child(axis_label(
                        format!("{height:.1} m"),
                        4.0,
                        PLOT_TOP - 7.0,
                        &theme,
                    ))
                    .child(axis_label(
                        format!("{:.1} m", height * 0.5),
                        4.0,
                        190.0 * 0.5 - 6.0,
                        &theme,
                    ))
                    .child(axis_label(
                        "0".to_string(),
                        24.0,
                        190.0 - PLOT_BOTTOM - 7.0,
                        &theme,
                    ))
                    .child(
                        div()
                            .absolute()
                            .right(px(PLOT_RIGHT))
                            .bottom(px(3.0))
                            .text_size(px(10.5))
                            .font_family(MONO)
                            .text_color(theme.muted_foreground)
                            .child(format!("{duration:.1} s")),
                    ),
            )
    }

    fn render_slope(&self, own: &PhysicsMaterial, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let other = partner(own, self.slope_against);
        let contact = own.contact_with(&other);
        let angle = self.angle.read(cx).value().end();
        let state = slope_state(contact.friction, angle, GRAVITY);
        let plot = SlopePlot {
            state,
            colors: Self::colors(cx),
        };

        let mono = |text: String, theme: &ui::Theme| {
            div()
                .text_xs()
                .font_family(MONO)
                .text_color(theme.foreground.opacity(0.9))
                .child(text)
        };
        let legend = |label: &'static str, color: Hsla, theme: &ui::Theme| {
            h_flex()
                .gap_1()
                .items_center()
                .child(div().w(px(10.0)).h(px(3.0)).rounded(px(2.0)).bg(color))
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
        };

        Self::card("Slope", "A block resting on an incline", cx)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(caption("Against", cx))
                    .child(self.against_picker(
                        "slope-against",
                        self.slope_against,
                        |this, i| this.slope_against = i,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(caption("Angle", cx))
                    .child(div().flex_1().child(Slider::new(&self.angle)))
                    .child(
                        div()
                            .w(px(52.0))
                            .child(mono(format!("{angle:.1}°"), &theme)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(if state.slipping {
                        Tag::danger().small().child("Slides").into_any_element()
                    } else {
                        Tag::success().small().child("Holds").into_any_element()
                    })
                    .child(mono(
                        format!(
                            "μ = {:.2} ({})",
                            contact.friction,
                            contact.friction_rule.label()
                        ),
                        &theme,
                    ))
                    .child(mono(
                        format!("holds up to {:.1}°", state.critical_deg),
                        &theme,
                    ))
                    .children(state.slipping.then(|| {
                        mono(
                            format!(
                                "a = {:.2} m/s², {:.1} m in 2 s",
                                state.acceleration,
                                slide_distance(&state, 2.0)
                            ),
                            &theme,
                        )
                    })),
            )
            .child(
                div().w_full().h(px(190.0)).child(
                    gpui::canvas(
                        |bounds, _window, _cx| bounds,
                        move |_bounds, bounds, window, _cx| plot.paint(bounds, window),
                    )
                    .size_full(),
                ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .child(legend("Gravity", theme.foreground, &theme))
                    .child(legend("Normal", theme.info, &theme))
                    .child(legend("Friction", theme.warning, &theme))
                    .child(legend("Net force", theme.danger, &theme))
                    .child(legend("Friction limit", theme.warning.opacity(0.6), &theme)),
            )
    }
}

fn axis_label(text: String, left: f32, top: f32, theme: &ui::Theme) -> Div {
    div()
        .absolute()
        .left(px(left.max(0.0)))
        .top(px(top))
        .w(px(PLOT_LEFT - 6.0))
        .text_size(px(10.5))
        .font_family(MONO)
        .text_color(theme.muted_foreground)
        .child(text)
}

impl Render for PreviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let material = self.doc.read(cx).asset().clone();

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(IconName::Play, "Preview", None, cx))
            .child(
                v_flex()
                    .id("material-preview-scroll")
                    .flex_1()
                    .min_h_0()
                    .scrollable(ScrollbarAxis::Vertical)
                    .child(
                        h_flex()
                            .w_full()
                            .p_3()
                            .gap_3()
                            .flex_wrap()
                            .items_start()
                            .child(self.render_bounce(&material, cx))
                            .child(self.render_slope(&material, cx)),
                    ),
            )
    }
}
