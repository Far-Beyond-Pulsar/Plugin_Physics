//! A second view of the same matrix: layers as nodes on a ring, a line for
//! every pair that collides. Easier than the grid for answering "what does
//! this layer touch?", and it makes a layer that touches everything (or
//! nothing) obvious at a glance.

use gpui::prelude::FluentBuilder;
use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::tag::Tag;
use ui::PixelsExt;
use ui::{h_flex, v_flex, ActiveTheme, IconName, Selectable, Sizable, StyledExt};

use crate::assets::layers::CollisionLayers;
use crate::editors::common::chrome::{
    caption, color_of, empty_message, panel_header, toolbar_divider,
};
use crate::editors::common::paint::{stroke_polyline, ChartColors};
use crate::editors::common::panel::dock_panel;

use super::state::{Doc, Sel};

/// How far out the ring reaches, as a fraction of half the panel.
const RING: f32 = 0.36;
const NODE: f32 = 28.0;

/// Where node `i` of `n` sits, as fractions of the panel (0..1 each way).
pub fn node_position(i: usize, n: usize) -> (f32, f32) {
    if n <= 1 {
        return (0.5, 0.5);
    }
    // Start at the top and go clockwise.
    let angle = std::f32::consts::TAU * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
    (0.5 + RING * angle.cos(), 0.5 + RING * angle.sin())
}

/// A line segment of the graph, ready to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub one_sided: bool,
}

/// The pairs to draw: every colliding pair, plus one-sided ones (flagged).
pub fn edges(set: &CollisionLayers) -> Vec<Edge> {
    let n = set.len();
    let mut out = Vec::new();
    for a in 0..n {
        for b in (a + 1)..n {
            if set.collides(a, b) {
                out.push(Edge {
                    a,
                    b,
                    one_sided: false,
                });
            } else if set.is_one_sided(a, b) {
                out.push(Edge {
                    a,
                    b,
                    one_sided: true,
                });
            }
        }
    }
    out
}

pub struct LayerGraph {
    doc: Doc,
    sel: Sel,
    /// Draw only the selected layer's edges.
    focus: bool,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

dock_panel!(LayerGraph, "physics-layers-graph", "Graph");

impl LayerGraph {
    pub fn new(doc: Doc, sel: Sel, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&sel, |_, _, cx| cx.notify());
        Self {
            doc,
            sel,
            focus: false,
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        }
    }
}

impl Render for LayerGraph {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let set = self.doc.read(cx).asset().clone();
        let n = set.len();
        let selected = self.sel.read(cx).current(n);
        let all_edges = edges(&set);
        let pair_count = all_edges.iter().filter(|e| !e.one_sided).count();
        let self_collisions: Vec<usize> = (0..n).filter(|&i| set.collides(i, i)).collect();

        let colors = ChartColors {
            grid: theme.border,
            axis: theme.muted_foreground,
            line: theme.foreground.opacity(0.28),
            fill: theme.primary.opacity(0.1),
            accent: theme.primary,
            text: theme.foreground,
            muted: theme.muted_foreground,
            good: theme.success,
            bad: theme.danger,
            info: theme.info,
            warn: theme.warning,
        };
        let draw_edges: Vec<Edge> = all_edges
            .iter()
            .copied()
            .filter(|e| !self.focus || selected.is_none_or(|s| e.a == s || e.b == s))
            .collect();
        let draw_selves: Vec<usize> = self_collisions
            .iter()
            .copied()
            .filter(|i| !self.focus || selected.is_none_or(|s| *i == s))
            .collect();

        let canvas = {
            let draw_edges = draw_edges.clone();
            gpui::canvas(
                |bounds, _window, _cx| bounds,
                move |_bounds, bounds, window, _cx| {
                    let (ox, oy) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
                    let (w, h) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
                    let at = |i: usize| {
                        let (fx, fy) = node_position(i, n);
                        (ox + fx * w, oy + fy * h)
                    };
                    let p = |(x, y): (f32, f32)| point(px(x), px(y));

                    // Quiet edges first so the selected layer's sit on top.
                    for pass in [false, true] {
                        for e in &draw_edges {
                            let touches = selected.is_some_and(|s| e.a == s || e.b == s);
                            if touches != pass {
                                continue;
                            }
                            let (color, width) = if e.one_sided {
                                (colors.warn.opacity(0.9), 1.6)
                            } else if touches {
                                (colors.accent.opacity(0.95), 2.2)
                            } else if selected.is_some() {
                                (colors.line.opacity(0.35), 1.0)
                            } else {
                                (colors.line, 1.2)
                            };
                            stroke_polyline(window, &[p(at(e.a)), p(at(e.b))], width, color);
                        }
                    }

                    // A layer that collides with itself gets a ring around its node.
                    for &i in &draw_selves {
                        let (cx_, cy_) = at(i);
                        let ring: Vec<Point<Pixels>> = (0..=24)
                            .map(|k| {
                                let a = std::f32::consts::TAU * k as f32 / 24.0;
                                p((cx_ + 21.0 * a.cos(), cy_ + 21.0 * a.sin()))
                            })
                            .collect();
                        let color = if selected == Some(i) {
                            colors.accent
                        } else {
                            colors.line
                        };
                        stroke_polyline(window, &ring, 1.6, color);
                    }
                },
            )
            .absolute()
            .inset_0()
            .size_full()
        };

        let nodes = (0..n).map(|i| {
            let (fx, fy) = node_position(i, n);
            let active = selected == Some(i);
            let sel = self.sel.clone();
            let color = color_of(set.color_of(i));
            let dim = self.focus
                && selected.is_some()
                && !active
                && !set.collides(selected.unwrap_or(i), i);
            div()
                .id(SharedString::from(format!("graph-node-{i}")))
                .absolute()
                .left(relative(fx))
                .top(relative(fy))
                .ml(px(-NODE * 0.5))
                .mt(px(-NODE * 0.5))
                .size(px(NODE))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(color.opacity(if dim { 0.35 } else { 1.0 }))
                .border_2()
                .border_color(if active {
                    theme.foreground
                } else {
                    theme.background
                })
                .shadow_sm()
                .text_size(px(11.0))
                .font_semibold()
                .text_color(theme.background)
                .on_click(move |_, _, cx| sel.update(cx, |s, cx| s.select(Some(i), cx)))
                .child(format!("{i}"))
                .child(
                    div()
                        .absolute()
                        .top(px(NODE + 4.0))
                        .left(px(-36.0))
                        .w(px(NODE + 72.0))
                        .text_center()
                        .text_xs()
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if dim {
                            theme.muted_foreground
                        } else {
                            theme.foreground
                        })
                        .truncate()
                        .child(set.layers[i].name.clone()),
                )
        });

        let focus_toggle = {
            let this = cx.entity();
            Button::new("graph-focus")
                .label("Focus selected")
                .ghost()
                .small()
                .selected(self.focus)
                .tooltip("Show only the selected layer's collisions")
                .on_click(move |_, _, cx| {
                    this.update(cx, |g, cx| {
                        g.focus = !g.focus;
                        cx.notify();
                    })
                })
        };

        let body: AnyElement = if n == 0 {
            div()
                .flex_1()
                .min_h_0()
                .child(empty_message(
                    IconName::Network,
                    "No layers",
                    "Add layers to see how they relate",
                    cx,
                ))
                .into_any_element()
        } else {
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .m_3()
                .child(canvas)
                .children(nodes)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(
                IconName::Network,
                "Relationships",
                Some(
                    Tag::secondary()
                        .small()
                        .child(format!("{pair_count} colliding pairs"))
                        .into_any_element(),
                ),
                cx,
            ))
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(theme.border.opacity(0.6))
                    .child(focus_toggle)
                    .child(toolbar_divider(cx))
                    .child(caption(
                        "A ring around a layer means it collides with itself.",
                        cx,
                    ))
                    .when(all_edges.iter().any(|e| e.one_sided), |bar| {
                        bar.child(Tag::warning().small().child("amber lines are one-sided"))
                    }),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{edges, node_position};
    use crate::assets::layers::{CollisionLayers, PRESETS};

    #[test]
    fn nodes_sit_on_a_ring_starting_at_the_top() {
        let (x, y) = node_position(0, 8);
        assert!(
            (x - 0.5).abs() < 1e-5 && y < 0.5,
            "the first node is at the top"
        );
        let (x2, y2) = node_position(4, 8);
        assert!(
            (x2 - 0.5).abs() < 1e-5 && y2 > 0.5,
            "the opposite node is at the bottom"
        );
        assert_eq!(node_position(0, 1), (0.5, 0.5));
        for i in 0..12 {
            let (x, y) = node_position(i, 12);
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
        }
    }

    #[test]
    fn edges_follow_the_matrix_and_flag_one_sided_pairs() {
        let set = PRESETS
            .iter()
            .find(|p| p.name == "Shooter")
            .unwrap()
            .build();
        let list = edges(&set);
        let pairs = (0..set.len())
            .flat_map(|a| ((a + 1)..set.len()).map(move |b| (a, b)))
            .filter(|&(a, b)| set.collides(a, b))
            .count();
        assert_eq!(list.len(), pairs);
        assert!(list.iter().all(|e| !e.one_sided));

        let mut set = CollisionLayers::default();
        set.add(Some("A"));
        set.collide_none();
        set.layers[0].collides_with.push("A".into());
        let list = edges(&set);
        assert_eq!(list.len(), 1);
        assert!(list[0].one_sided);
    }
}
