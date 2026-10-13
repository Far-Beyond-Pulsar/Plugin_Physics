//! Left dock panel of the layers editor: the layers, in order.
//!
//! A layer's position is its bit in the engine's group masks, so the list
//! shows that index next to each name, and reordering is an edit like any
//! other.

use gpui::prelude::FluentBuilder;
use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::input::{InputEvent, InputState, TextInput};
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt};

use crate::assets::layers::MAX_LAYERS;
use crate::assets::Severity;
use crate::editors::common::chrome::{color_of, empty_message, panel_header, severity_color, severity_icon, swatch, MONO};
use crate::editors::common::panel::dock_panel;

use super::state::{add_layer, duplicate_layer, move_layer, remove_layer, Doc, Sel};

pub struct LayersList {
    doc: Doc,
    sel: Sel,
    search: Entity<InputState>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

dock_panel!(LayersList, "physics-layers-list", "Layers");

impl LayersList {
    pub fn new(doc: Doc, sel: Sel, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter layers…"));
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |_this, _i, event: &InputEvent, _w, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.observe(&sel, |_, _, cx| cx.notify()),
        ];
        Self { doc, sel, search, focus_handle: cx.focus_handle(), _subscriptions: subscriptions }
    }

    /// Up / Down select, Delete removes, Ctrl+D duplicates, Alt+Up/Down reorders.
    fn on_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let len = self.doc.read(cx).asset().len();
        if len == 0 {
            return;
        }
        let current = self.sel.read(cx).current(len);
        let mods = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "up" | "down" => {
                let down = event.keystroke.key == "down";
                match (current, mods.alt) {
                    (Some(i), true) => move_layer(&self.doc, &self.sel, i, if down { 1 } else { -1 }, cx),
                    (Some(i), false) => {
                        let next = if down { (i + 1).min(len - 1) } else { i.saturating_sub(1) };
                        self.sel.update(cx, |s, cx| s.select(Some(next), cx));
                    }
                    (None, _) => self.sel.update(cx, |s, cx| s.select(Some(0), cx)),
                }
            }
            "delete" | "backspace" => {
                if let Some(i) = current {
                    remove_layer(&self.doc, &self.sel, i, cx);
                }
            }
            "d" if mods.secondary() => {
                if let Some(i) = current {
                    duplicate_layer(&self.doc, &self.sel, i, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for LayersList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let doc = self.doc.read(cx);
        let set = doc.asset();
        let len = set.len();
        let selected = self.sel.read(cx).current(len);
        let query = self.search.read(cx).value().trim().to_lowercase();
        let issues = set.validate();

        let rows: Vec<AnyElement> = (0..len)
            .filter(|&i| query.is_empty() || set.layers[i].name.to_lowercase().contains(&query))
            .map(|i| {
                let layer = &set.layers[i];
                let active = selected == Some(i);
                let worst = issues.iter().filter(|x| x.subject == layer.name).map(|x| x.severity).max();
                let count = set.collision_count(i);
                let (select_sel, up_sel, down_sel, dup_sel, del_sel) =
                    (self.sel.clone(), self.sel.clone(), self.sel.clone(), self.sel.clone(), self.sel.clone());
                let (up_doc, down_doc, dup_doc, del_doc) =
                    (self.doc.clone(), self.doc.clone(), self.doc.clone(), self.doc.clone());
                let group: SharedString = format!("layer-row-{i}").into();

                h_flex()
                    .id(SharedString::from(format!("layer-{i}")))
                    .group(group.clone())
                    .relative()
                    .w_full()
                    .h(px(34.0))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .bg(if active { theme.list_active } else { theme.transparent })
                    .hover(|s| s.bg(theme.list_hover))
                    .when(active, |row| {
                        row.child(div().absolute().left_0().top_0().bottom_0().w(px(3.0)).bg(theme.primary))
                    })
                    .on_click(move |_, _, cx| select_sel.update(cx, |s, cx| s.select(Some(i), cx)))
                    .child(
                        div()
                            .w(px(22.0))
                            .text_size(px(10.5))
                            .font_family(MONO)
                            .text_color(theme.muted_foreground)
                            .child(format!("{i}")),
                    )
                    .child(swatch(color_of(set.color_of(i)), 14.0, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                            .text_color(theme.foreground)
                            .child(if layer.name.is_empty() { "(unnamed)".to_string() } else { layer.name.clone() }),
                    )
                    .children(worst.filter(|w| *w >= Severity::Warning).map(|w| {
                        Icon::new(severity_icon(w)).size(px(12.0)).text_color(severity_color(w, cx))
                    }))
                    .child(
                        h_flex()
                            .gap_0p5()
                            .opacity(0.0)
                            .when(active, |h| h.opacity(1.0))
                            .group_hover(group, |s| s.opacity(1.0))
                            .child(
                                Button::new(SharedString::from(format!("layer-up-{i}")))
                                    .icon(IconName::ArrowUp)
                                    .ghost()
                                    .xsmall()
                                    .tooltip("Move up (Alt+Up)")
                                    .on_click(move |_, _, cx| move_layer(&up_doc, &up_sel, i, -1, cx)),
                            )
                            .child(
                                Button::new(SharedString::from(format!("layer-down-{i}")))
                                    .icon(IconName::ArrowDown)
                                    .ghost()
                                    .xsmall()
                                    .tooltip("Move down (Alt+Down)")
                                    .on_click(move |_, _, cx| move_layer(&down_doc, &down_sel, i, 1, cx)),
                            )
                            .child(
                                Button::new(SharedString::from(format!("layer-dup-{i}")))
                                    .icon(IconName::Copy)
                                    .ghost()
                                    .xsmall()
                                    .tooltip("Duplicate (Ctrl+D)")
                                    .on_click(move |_, _, cx| duplicate_layer(&dup_doc, &dup_sel, i, cx)),
                            )
                            .child(
                                Button::new(SharedString::from(format!("layer-del-{i}")))
                                    .icon(IconName::Trash)
                                    .ghost()
                                    .xsmall()
                                    .tooltip("Delete (Del)")
                                    .on_click(move |_, _, cx| remove_layer(&del_doc, &del_sel, i, cx)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(24.0))
                            .text_right()
                            .text_xs()
                            .font_family(MONO)
                            .text_color(theme.muted_foreground)
                            .tooltip({
                                move |window, cx| {
                                    ui::tooltip::Tooltip::new(format!("Collides with {count} layer(s)")).build(window, cx)
                                }
                            })
                            .child(count.to_string()),
                    )
                    .into_any_element()
            })
            .collect();

        let at_limit = len >= MAX_LAYERS;
        let (add_doc, add_sel) = (self.doc.clone(), self.sel.clone());

        let body = if rows.is_empty() {
            let (title, hint) = if len == 0 {
                ("No layers", "Add a layer to start building the collision matrix")
            } else {
                ("No matches", "No layer name contains that text")
            };
            div().flex_1().min_h_0().child(empty_message(IconName::List, title, hint, cx)).into_any_element()
        } else {
            v_flex()
                .id("layers-scroll")
                .flex_1()
                .min_h_0()
                .py_1()
                .scrollable(ScrollbarAxis::Vertical)
                .children(rows)
                .into_any_element()
        };

        v_flex()
            .id("physics-layers-list")
            .size_full()
            .bg(theme.sidebar)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.on_key_down(e, cx)))
            .child(panel_header(
                IconName::List,
                "Layers",
                Some(
                    if at_limit {
                        Tag::warning().small().child(format!("{len} / {MAX_LAYERS}"))
                    } else {
                        Tag::secondary().small().child(format!("{len} / {MAX_LAYERS}"))
                    }
                    .into_any_element(),
                ),
                cx,
            ))
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .items_center()
                    .border_b_1()
                    .border_color(theme.border.opacity(0.6))
                    .child(div().flex_1().min_w_0().child(
                        TextInput::new(&self.search).small().cleanable().prefix(
                            Icon::new(IconName::Search).size_3p5().text_color(theme.muted_foreground),
                        ),
                    ))
                    .child(
                        Button::new("layers-add")
                            .icon(IconName::Plus)
                            .label("Add")
                            .primary()
                            .small()
                            .disabled(at_limit)
                            .tooltip(if at_limit { "The engine's collision groups hold 32 layers" } else { "Add a layer" })
                            .on_click(move |_, _, cx| add_layer(&add_doc, &add_sel, cx)),
                    ),
            )
            .child(body)
    }
}
