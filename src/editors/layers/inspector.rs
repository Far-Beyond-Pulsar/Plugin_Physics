//! Right dock panel of the layers editor: everything about the selected
//! layer, including exactly what the engine will see of it.

use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::checkbox::Checkbox;
use ui::input::{InputEvent, InputState, TextInput};
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt};

use crate::assets::layers::{RowOp, MAX_LAYERS};
use crate::assets::{auto_color, hex_color, parse_hex_color};
use crate::editors::common::chrome::{caption, color_of, empty_message, panel_header, section, swatch, MONO};
use crate::editors::common::panel::dock_panel;

use super::state::{duplicate_layer, move_layer, remove_layer, Doc, Sel};

/// Quick color choices under the hex field.
const PALETTE_SIZE: usize = 10;

pub struct LayerInspector {
    doc: Doc,
    sel: Sel,
    name: Entity<InputState>,
    color: Entity<InputState>,
    description: Entity<InputState>,
    name_error: Option<&'static str>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

dock_panel!(LayerInspector, "physics-layers-inspector", "Inspector");

impl LayerInspector {
    pub fn new(doc: Doc, sel: Sel, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Layer name"));
        let color = cx.new(|cx| InputState::new(window, cx).placeholder("#rrggbb"));
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("What lives on this layer?"));

        let mut subscriptions = vec![cx.observe(&sel, |_, _, cx| cx.notify())];

        let (d, s) = (doc.clone(), sel.clone());
        subscriptions.push(cx.subscribe_in(&name, window, move |this: &mut Self, input, event: &InputEvent, _w, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let len = d.read(cx).asset().len();
            let Some(i) = s.read(cx).current(len) else { return };
            let text = input.read(cx).value().to_string();
            if d.read(cx).asset().layers[i].name == text {
                this.name_error = None;
                return;
            }
            let mut result = Ok(());
            d.update(cx, |d, cx| d.edit("Rename layer", Some("rename"), cx, |set| result = set.rename(i, &text)));
            this.name_error = result.err().map(|e| e.message());
            cx.notify();
        }));

        let (d, s) = (doc.clone(), sel.clone());
        subscriptions.push(cx.subscribe_in(&color, window, move |_this: &mut Self, input, event: &InputEvent, _w, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let len = d.read(cx).asset().len();
            let Some(i) = s.read(cx).current(len) else { return };
            let text = input.read(cx).value().trim().to_string();
            if let Some(rgb) = parse_hex_color(&text) {
                let hex = hex_color(rgb);
                d.update(cx, |d, cx| {
                    d.edit("Change layer color", Some("color"), cx, |set| set.layers[i].color = Some(hex))
                });
            }
        }));

        let (d, s) = (doc.clone(), sel.clone());
        subscriptions.push(cx.subscribe_in(&description, window, move |_this: &mut Self, input, event: &InputEvent, _w, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let len = d.read(cx).asset().len();
            let Some(i) = s.read(cx).current(len) else { return };
            let text = input.read(cx).value().to_string();
            if d.read(cx).asset().layers[i].description != text {
                d.update(cx, |d, cx| {
                    d.edit("Edit layer notes", Some("layer-notes"), cx, |set| set.layers[i].description = text)
                });
            }
        }));

        Self { doc, sel, name, color, description, name_error: None, focus_handle: cx.focus_handle(), _subscriptions: subscriptions }
    }

    /// Show the selected layer's values in the text fields, unless one is
    /// being typed in.
    fn sync_fields(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let layer = self.doc.read(cx).asset().layers[i].clone();
        let hex = layer.color.clone().unwrap_or_default();
        for (input, text) in [(&self.name, layer.name), (&self.color, hex), (&self.description, layer.description)] {
            let focused = input.read(cx).focus_handle(cx).contains_focused(window, cx);
            if !focused && input.read(cx).value().as_ref() != text.as_str() {
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
        if !self.name.read(cx).focus_handle(cx).contains_focused(window, cx) {
            self.name_error = None;
        }
    }

    fn render_identity(&self, i: usize, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let set = self.doc.read(cx).asset();
        let current = set.color_of(i);
        let palette = (0..PALETTE_SIZE).map(|n| {
            let rgb = auto_color(n);
            let doc = self.doc.clone();
            div()
                .id(SharedString::from(format!("palette-{n}")))
                .size(px(18.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .bg(color_of(rgb))
                .border_1()
                .border_color(if rgb == current { theme.foreground } else { theme.foreground.opacity(0.15) })
                .on_click(move |_, _, cx| {
                    let hex = hex_color(rgb);
                    doc.update(cx, |d, cx| {
                        d.edit("Change layer color", None, cx, |set| set.layers[i].color = Some(hex))
                    });
                })
        });
        let auto_doc = self.doc.clone();

        v_flex()
            .w_full()
            .p_3()
            .gap_2p5()
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .child(
                v_flex()
                    .gap_1()
                    .child(caption("Name", cx))
                    .child(TextInput::new(&self.name).small())
                    .children(self.name_error.map(|e| {
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(Icon::new(IconName::TriangleAlert).size(px(11.0)).text_color(theme.danger))
                            .child(div().text_xs().text_color(theme.danger).child(e))
                    })),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(caption("Color", cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(swatch(color_of(current), 24.0, cx))
                            .child(div().flex_1().child(TextInput::new(&self.color).small()))
                            .child(
                                Button::new("layer-color-auto")
                                    .label("Auto")
                                    .ghost()
                                    .small()
                                    .tooltip("Pick a color from the layer's position")
                                    .on_click(move |_, _, cx| {
                                        auto_doc.update(cx, |d, cx| {
                                            d.edit("Reset layer color", None, cx, |set| set.layers[i].color = None)
                                        })
                                    }),
                            ),
                    )
                    .child(h_flex().gap_1().flex_wrap().children(palette)),
            )
            .child(v_flex().gap_1().child(caption("Notes", cx)).child(TextInput::new(&self.description).small()))
    }

    fn render_engine_view(&self, i: usize, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let set = self.doc.read(cx).asset();
        let (bit, mask) = (set.membership_bit(i), set.filter_mask(i));
        let binary = |value: u32| {
            (0..4)
                .rev()
                .map(|b| format!("{:08b}", (value >> (b * 8)) & 0xFF))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let row = |label: &'static str, value: String| {
            h_flex()
                .gap_2()
                .items_baseline()
                .child(div().w(px(76.0)).text_xs().text_color(theme.muted_foreground).child(label))
                .child(div().text_xs().font_family(MONO).text_color(theme.foreground.opacity(0.92)).child(value))
        };
        section(
            "What the engine sees",
            Some("Collision groups are 32-bit masks. This layer's position is its bit.".into()),
            v_flex()
                .gap_1()
                .child(row("Layer bit", format!("{i}  (1 << {i})")))
                .child(row("Group", format!("0x{bit:08X}")))
                .child(row("Collides", format!("0x{mask:08X}")))
                .child(row("", binary(mask))),
            cx,
        )
    }

    fn render_partners(&self, i: usize, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let set = self.doc.read(cx).asset();
        let n = set.len();

        let op_button = |id: &'static str, label: &'static str, tip: &'static str, op: RowOp| {
            let doc = self.doc.clone();
            Button::new(id)
                .label(label)
                .ghost()
                .xsmall()
                .tooltip(tip)
                .on_click(move |_, _, cx| doc.update(cx, |d, cx| d.edit("Change collisions", None, cx, |s| s.apply_row(i, op))))
        };

        let rows = (0..n).map(|j| {
            let doc = self.doc.clone();
            let colliding = set.collides(i, j);
            let one_sided = set.is_one_sided(i, j);
            h_flex()
                .id(SharedString::from(format!("partner-{j}")))
                .w_full()
                .h(px(28.0))
                .px_1()
                .gap_2()
                .items_center()
                .rounded(px(5.0))
                .hover(|s| s.bg(theme.list_hover))
                .child(
                    Checkbox::new(SharedString::from(format!("partner-check-{j}")))
                        .checked(colliding)
                        .on_click(move |checked, _, cx| {
                            let on = *checked;
                            doc.update(cx, |d, cx| d.edit("Toggle collision", None, cx, |s| s.set_pair(i, j, on)))
                        }),
                )
                .child(swatch(color_of(set.color_of(j)), 12.0, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(if j == i { format!("{} (itself)", set.layers[j].name) } else { set.layers[j].name.clone() }),
                )
                .children(one_sided.then(|| Tag::warning().small().child("one-sided")))
        });

        section(
            "Collides with",
            Some(format!("{} of {n} layers", set.collision_count(i)).into()),
            v_flex()
                .gap_1p5()
                .child(
                    h_flex()
                        .gap_1()
                        .child(op_button("partners-all", "All", "Collide with every layer", RowOp::CollideWithAll))
                        .child(op_button("partners-none", "None", "Collide with no layer", RowOp::CollideWithNone))
                        .child(op_button("partners-invert", "Invert", "Flip every pairing", RowOp::Invert)),
                )
                .child(v_flex().children(rows)),
            cx,
        )
    }

    fn render_actions(&self, i: usize, cx: &App) -> impl IntoElement {
        let set = self.doc.read(cx).asset();
        let (n, theme) = (set.len(), cx.theme());
        let (up_d, up_s, down_d, down_s, dup_d, dup_s, del_d, del_s) = (
            self.doc.clone(),
            self.sel.clone(),
            self.doc.clone(),
            self.sel.clone(),
            self.doc.clone(),
            self.sel.clone(),
            self.doc.clone(),
            self.sel.clone(),
        );
        h_flex()
            .w_full()
            .p_3()
            .gap_1()
            .flex_wrap()
            .border_t_1()
            .border_color(theme.border.opacity(0.6))
            .child(
                Button::new("inspector-up")
                    .icon(IconName::ArrowUp)
                    .label("Up")
                    .ghost()
                    .small()
                    .disabled(i == 0)
                    .on_click(move |_, _, cx| move_layer(&up_d, &up_s, i, -1, cx)),
            )
            .child(
                Button::new("inspector-down")
                    .icon(IconName::ArrowDown)
                    .label("Down")
                    .ghost()
                    .small()
                    .disabled(i + 1 >= n)
                    .on_click(move |_, _, cx| move_layer(&down_d, &down_s, i, 1, cx)),
            )
            .child(
                Button::new("inspector-duplicate")
                    .icon(IconName::Copy)
                    .label("Duplicate")
                    .ghost()
                    .small()
                    .disabled(n >= MAX_LAYERS)
                    .on_click(move |_, _, cx| duplicate_layer(&dup_d, &dup_s, i, cx)),
            )
            .child(
                Button::new("inspector-delete")
                    .icon(IconName::Trash)
                    .label("Delete")
                    .ghost()
                    .small()
                    .on_click(move |_, _, cx| remove_layer(&del_d, &del_s, i, cx)),
            )
    }
}

impl Render for LayerInspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let len = self.doc.read(cx).asset().len();
        let selected = self.sel.read(cx).current(len);

        let (title, badge, body): (String, Option<AnyElement>, AnyElement) = match selected {
            None => (
                "Inspector".to_string(),
                None,
                div()
                    .flex_1()
                    .min_h_0()
                    .child(empty_message(
                        IconName::List,
                        "No layer selected",
                        "Pick a layer in the list, the matrix or the graph",
                        cx,
                    ))
                    .into_any_element(),
            ),
            Some(i) => {
                self.sync_fields(i, window, cx);
                let name = self.doc.read(cx).asset().layers[i].name.clone();
                (
                    name,
                    Some(Tag::secondary().small().child(format!("bit {i}")).into_any_element()),
                    v_flex()
                        .id("layer-inspector-scroll")
                        .flex_1()
                        .min_h_0()
                        .scrollable(ScrollbarAxis::Vertical)
                        .child(self.render_identity(i, cx))
                        .child(self.render_engine_view(i, cx))
                        .child(self.render_partners(i, cx))
                        .child(self.render_actions(i, cx))
                        .into_any_element(),
                )
            }
        };

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(IconName::Settings, title, badge, cx))
            .child(body)
    }
}
