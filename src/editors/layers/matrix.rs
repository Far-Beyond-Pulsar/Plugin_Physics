//! Center dock panel of the layers editor: who collides with whom.
//!
//! A triangular grid (the relation is symmetric, so one half says it all).
//! Clicking a cell flips the pair on both sides. A pair that only one side
//! lists does nothing in the engine, so it is drawn in amber instead of
//! blending in: it is almost always a mistake.

use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::scroll::ScrollbarAxis;
use ui::{h_flex, v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt};

use crate::assets::layers::Symmetry;
use crate::editors::common::chrome::{caption, color_of, empty_message, panel_header, toolbar_divider, MONO};
use crate::editors::common::panel::dock_panel;

use super::state::{Doc, Sel};

const CELL: f32 = 26.0;
const GAP: f32 = 2.0;
const NAME_WIDTH: f32 = 172.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellState {
    Collide,
    Ignore,
    /// One layer lists the other and not the other way round.
    OneSided,
}

pub struct CollisionMatrix {
    doc: Doc,
    sel: Sel,
    hover: Option<(usize, usize)>,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

dock_panel!(CollisionMatrix, "physics-layers-matrix", "Matrix");

impl CollisionMatrix {
    pub fn new(doc: Doc, sel: Sel, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&sel, |_, _, cx| cx.notify());
        Self { doc, sel, hover: None, focus_handle: cx.focus_handle(), _subscription: subscription }
    }

    fn legend(label: &'static str, fill: Hsla, border: Hsla, cx: &App) -> impl IntoElement {
        h_flex()
            .gap_1()
            .items_center()
            .child(div().size(px(12.0)).rounded(px(3.0)).bg(fill).border_1().border_color(border))
            .child(caption(label, cx))
    }
}

impl Render for CollisionMatrix {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let set = self.doc.read(cx).asset().clone();
        let n = set.len();
        let selected = self.sel.read(cx).current(n);
        let one_sided = (0..n).any(|i| ((i + 1)..n).any(|j| set.is_one_sided(i, j)));

        let (all_doc, none_doc, union_doc, inter_doc) =
            (self.doc.clone(), self.doc.clone(), self.doc.clone(), self.doc.clone());
        let toolbar = h_flex()
            .w_full()
            .flex_shrink_0()
            .px_3()
            .py_1p5()
            .gap_1p5()
            .items_center()
            .flex_wrap()
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .child(
                Button::new("matrix-all")
                    .label("Collide all")
                    .ghost()
                    .small()
                    .tooltip("Every layer collides with every layer")
                    .on_click(move |_, _, cx| all_doc.update(cx, |d, cx| d.edit("Collide all", None, cx, |s| s.collide_all()))),
            )
            .child(
                Button::new("matrix-none")
                    .label("Collide none")
                    .ghost()
                    .small()
                    .tooltip("No layer collides with any layer")
                    .on_click(move |_, _, cx| none_doc.update(cx, |d, cx| d.edit("Collide none", None, cx, |s| s.collide_none()))),
            )
            .child(toolbar_divider(cx))
            .child(caption("One-sided pairs:", cx))
            .child(
                Button::new("matrix-union")
                    .label("Make them collide")
                    .ghost()
                    .small()
                    .disabled(!one_sided)
                    .on_click(move |_, _, cx| {
                        union_doc.update(cx, |d, cx| d.edit("Resolve one-sided pairs", None, cx, |s| s.make_symmetric(Symmetry::Union)))
                    }),
            )
            .child(
                Button::new("matrix-intersection")
                    .label("Clear them")
                    .ghost()
                    .small()
                    .disabled(!one_sided)
                    .on_click(move |_, _, cx| {
                        inter_doc.update(cx, |d, cx| d.edit("Resolve one-sided pairs", None, cx, |s| s.make_symmetric(Symmetry::Intersection)))
                    }),
            )
            .child(div().flex_1())
            .child(Self::legend("collide", theme.primary.opacity(0.85), theme.primary, cx))
            .child(Self::legend("ignore", theme.foreground.opacity(0.06), theme.border, cx))
            .child(Self::legend("one-sided", theme.warning.opacity(0.35), theme.warning, cx));

        let body: AnyElement = if n == 0 {
            div()
                .flex_1()
                .min_h_0()
                .child(empty_message(IconName::Table, "No layers", "Add layers to fill in the collision matrix", cx))
                .into_any_element()
        } else {
            // Column headers: the layer's index over its color, name on hover.
            let headers = h_flex().gap(px(GAP)).pl(px(NAME_WIDTH + GAP)).children((0..n).map(|j| {
                let name = set.layers[j].name.clone();
                let hot = self.hover.is_some_and(|(_, c)| c == j) || selected == Some(j);
                v_flex()
                    .id(SharedString::from(format!("col-{j}")))
                    .w(px(CELL))
                    .items_center()
                    .gap_0p5()
                    .tooltip(move |window, cx| ui::tooltip::Tooltip::new(name.clone()).build(window, cx))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_family(MONO)
                            .text_color(if hot { theme.foreground } else { theme.muted_foreground })
                            .child(format!("{j}")),
                    )
                    .child(div().w(px(CELL - 6.0)).h(px(3.0)).rounded(px(2.0)).bg(color_of(set.color_of(j))))
            }));

            let rows = (0..n).map(|i| {
                let layer = &set.layers[i];
                let row_hot = self.hover.is_some_and(|(r, _)| r == i) || selected == Some(i);
                let sel_for_name = self.sel.clone();
                let name = h_flex()
                    .id(SharedString::from(format!("row-{i}")))
                    .w(px(NAME_WIDTH))
                    .flex_shrink_0()
                    .h(px(CELL))
                    .px_1p5()
                    .gap_1p5()
                    .items_center()
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .bg(if row_hot { theme.foreground.opacity(0.07) } else { theme.transparent })
                    .on_click(move |_, _, cx| sel_for_name.update(cx, |s, cx| s.select(Some(i), cx)))
                    .child(div().size(px(10.0)).flex_shrink_0().rounded(px(3.0)).bg(color_of(set.color_of(i))))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_weight(if selected == Some(i) { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                            .text_color(theme.foreground)
                            .child(layer.name.clone()),
                    )
                    .child(div().text_size(px(10.5)).font_family(MONO).text_color(theme.muted_foreground).child(format!("{i}")));

                let cells = (0..=i).map(|j| {
                    let state = if set.is_one_sided(i, j) {
                        CellState::OneSided
                    } else if set.collides(i, j) {
                        CellState::Collide
                    } else {
                        CellState::Ignore
                    };
                    // Both layers of the hovered pair light up wherever they appear.
                    let cross = self.hover.is_some_and(|(r, c)| i == r || j == r || i == c || j == c);
                    let doc = self.doc.clone();
                    let (a, b) = (set.layers[i].name.clone(), set.layers[j].name.clone());
                    let what = match state {
                        CellState::Collide => "collide",
                        CellState::Ignore => "ignore each other",
                        CellState::OneSided => "are listed by one side only (no collision)",
                    };
                    let tip = format!("{a} and {b} {what}. Click to toggle.");

                    div()
                        .id(SharedString::from(format!("cell-{i}-{j}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(CELL))
                        .flex_shrink_0()
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .bg(match state {
                            CellState::Collide => theme.primary.opacity(if cross { 1.0 } else { 0.8 }),
                            CellState::Ignore => theme.foreground.opacity(if cross { 0.12 } else { 0.05 }),
                            CellState::OneSided => theme.warning.opacity(0.35),
                        })
                        .border_1()
                        .border_color(match state {
                            CellState::OneSided => theme.warning,
                            _ if self.hover == Some((i, j)) => theme.foreground.opacity(0.7),
                            _ => theme.border.opacity(0.5),
                        })
                        .tooltip(move |window, cx| ui::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            let was = this.hover;
                            if *hovered {
                                this.hover = Some((i, j));
                            } else if this.hover == Some((i, j)) {
                                this.hover = None;
                            }
                            if was != this.hover {
                                cx.notify();
                            }
                        }))
                        .on_click(move |_, _, cx| {
                            doc.update(cx, |d, cx| d.edit("Toggle collision", None, cx, |s| s.toggle_pair(i, j)))
                        })
                        .child(match state {
                            CellState::Collide => Icon::new(IconName::Check).size(px(13.0)).text_color(theme.primary_foreground).into_any_element(),
                            CellState::OneSided => Icon::new(IconName::TriangleAlert).size(px(12.0)).text_color(theme.warning).into_any_element(),
                            CellState::Ignore => div().into_any_element(),
                        })
                });
                h_flex().gap(px(GAP)).items_center().child(name).children(cells)
            });

            v_flex()
                .id("matrix-scroll")
                .flex_1()
                .min_h_0()
                .p_3()
                .gap(px(GAP))
                .scrollable(ScrollbarAxis::Both)
                .child(headers)
                .children(rows)
                .into_any_element()
        };

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(IconName::Table, "Collision matrix", None, cx))
            .child(toolbar)
            .child(body)
    }
}
