//! Bottom dock of the layers editor: what is wrong with the layer set, and
//! the table of masks the engine will actually use.

use gpui::*;
use ui::button::{Button, ButtonVariants};
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, Disableable, IconName, Sizable, StyledExt};

use crate::assets::layers::{CollisionLayers, LayerFix, LayerIssue, MAX_LAYERS};
use crate::assets::Severity;
use crate::editors::common::chrome::{
    all_clear, color_of, empty_message, issue_row, panel_header, swatch, MONO,
};
use crate::editors::common::panel::dock_panel;

use super::state::{Doc, Sel};

/// The ways to repair `issue`. A one-sided pair can be settled either way,
/// so it offers both; the validator's own suggestion (the one that keeps
/// today's behavior) is last.
pub fn fixes_for(issue: &LayerIssue) -> Vec<(String, LayerFix)> {
    match &issue.fix {
        Some(LayerFix::SymmetrizePair { a, b, .. }) => vec![
            (
                "Make both collide".to_string(),
                LayerFix::SymmetrizePair {
                    a: a.clone(),
                    b: b.clone(),
                    collide: true,
                },
            ),
            (
                "Make neither collide".to_string(),
                LayerFix::SymmetrizePair {
                    a: a.clone(),
                    b: b.clone(),
                    collide: false,
                },
            ),
        ],
        Some(fix) => vec![(fix.label(), fix.clone())],
        None => Vec::new(),
    }
}

/// Apply every issue's own suggested fix, repeating until nothing more can be
/// repaired (one repair can reveal or clear others).
pub fn fix_all(set: &mut CollisionLayers) {
    for _ in 0..8 {
        let fixes: Vec<LayerFix> = set.validate().into_iter().filter_map(|i| i.fix).collect();
        if fixes.is_empty() {
            break;
        }
        for fix in fixes {
            set.apply_fix(&fix);
        }
    }
}

// ── Issues ─────────────────────────────────────────────────────────────────

pub struct IssuesPanel {
    doc: Doc,
    sel: Sel,
    focus_handle: FocusHandle,
}

dock_panel!(IssuesPanel, "physics-layers-issues", "Issues");

impl IssuesPanel {
    pub fn new(doc: Doc, sel: Sel, cx: &mut Context<Self>) -> Self {
        Self {
            doc,
            sel,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for IssuesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let set = self.doc.read(cx).asset().clone();
        let mut issues = set.validate();
        issues.sort_by_key(|i| std::cmp::Reverse(i.severity));
        let count = |s: Severity| issues.iter().filter(|i| i.severity == s).count();
        let (errors, warnings, notes) = (
            count(Severity::Error),
            count(Severity::Warning),
            count(Severity::Info),
        );
        let fixable = issues.iter().any(|i| i.fix.is_some());

        let doc = self.doc.clone();
        let fix_all_doc = self.doc.clone();
        let sel = self.sel.clone();

        let rows: Vec<AnyElement> = issues
            .iter()
            .enumerate()
            .map(|(n, issue)| {
                let doc = doc.clone();
                let sel = sel.clone();
                let subject = issue.subject.clone();
                issue_row(
                    SharedString::from(format!("layer-issue-{n}")),
                    issue,
                    fixes_for(issue),
                    move |fix, _, cx| {
                        doc.update(cx, |d, cx| {
                            d.edit("Fix layers", None, cx, |set| set.apply_fix(&fix))
                        });
                        // Point the other panels at the layer the problem was about.
                        let index = doc.read(cx).asset().index_of(&subject);
                        if index.is_some() {
                            sel.update(cx, |s, cx| s.select(index, cx));
                        }
                    },
                    cx,
                )
                .into_any_element()
            })
            .collect();

        let body = if rows.is_empty() {
            div()
                .w_full()
                .p_3()
                .child(all_clear(
                    "No problems: every pairing is mutual and every name is unique",
                    cx,
                ))
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .p_3()
                .gap_1p5()
                .children(rows)
                .into_any_element()
        };

        let tags = h_flex()
            .gap_1()
            .child(
                Tag::secondary()
                    .small()
                    .child(format!("{} / {MAX_LAYERS} layers", set.len())),
            )
            .children((errors > 0).then(|| Tag::danger().small().child(format!("{errors} errors"))))
            .children(
                (warnings > 0)
                    .then(|| Tag::warning().small().child(format!("{warnings} warnings"))),
            )
            .children((notes > 0).then(|| Tag::info().small().child(format!("{notes} notes"))));

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(
                IconName::TriangleAlert,
                "Issues",
                Some(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(tags)
                        .child(
                            Button::new("layers-fix-all")
                                .label("Fix all")
                                .small()
                                .disabled(!fixable)
                                .tooltip("Apply every suggested repair")
                                .on_click(move |_, _, cx| {
                                    fix_all_doc.update(cx, |d, cx| {
                                        d.edit("Fix all layer issues", None, cx, fix_all)
                                    })
                                }),
                        )
                        .into_any_element(),
                ),
                cx,
            ))
            .child(
                v_flex()
                    .id("layer-issues-scroll")
                    .flex_1()
                    .min_h_0()
                    .scrollable(ScrollbarAxis::Vertical)
                    .child(body),
            )
    }
}

// ── Masks ──────────────────────────────────────────────────────────────────

pub struct MasksPanel {
    doc: Doc,
    focus_handle: FocusHandle,
}

dock_panel!(MasksPanel, "physics-layers-masks", "Masks");

impl MasksPanel {
    pub fn new(doc: Doc, cx: &mut Context<Self>) -> Self {
        Self {
            doc,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for MasksPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let set = self.doc.read(cx).asset().clone();
        let n = set.len().min(MAX_LAYERS);

        let (rust, table) = (set.to_rust_consts(), set.masks_table());
        let header = |label: &'static str, width: f32| {
            div()
                .w(px(width))
                .text_size(px(10.5))
                .font_semibold()
                .text_color(theme.muted_foreground)
                .child(label)
        };

        let rows = (0..n).map(|i| {
            let strip = (0..n).map(|j| {
                div()
                    .w(px(8.0))
                    .h(px(14.0))
                    .rounded(px(2.0))
                    .bg(if set.collides(i, j) {
                        color_of(set.color_of(j))
                    } else {
                        theme.foreground.opacity(0.07)
                    })
            });
            h_flex()
                .w_full()
                .h(px(28.0))
                .px_3()
                .gap_2()
                .items_center()
                .hover(|s| s.bg(theme.list_hover))
                .child(
                    div()
                        .w(px(30.0))
                        .text_xs()
                        .font_family(MONO)
                        .text_color(theme.muted_foreground)
                        .child(format!("{i}")),
                )
                .child(
                    h_flex()
                        .w(px(170.0))
                        .gap_2()
                        .items_center()
                        .child(swatch(color_of(set.color_of(i)), 12.0, cx))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.foreground)
                                .truncate()
                                .child(set.layers[i].name.clone()),
                        ),
                )
                .child(
                    div()
                        .w(px(96.0))
                        .text_xs()
                        .font_family(MONO)
                        .text_color(theme.foreground.opacity(0.9))
                        .child(format!("0x{:08X}", set.membership_bit(i))),
                )
                .child(
                    div()
                        .w(px(96.0))
                        .text_xs()
                        .font_family(MONO)
                        .text_color(theme.foreground.opacity(0.9))
                        .child(format!("0x{:08X}", set.filter_mask(i))),
                )
                .child(h_flex().gap(px(2.0)).children(strip))
        });

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(panel_header(
                IconName::Table,
                "Masks",
                Some(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("masks-copy-rust")
                                .label("Copy as Rust")
                                .ghost()
                                .small()
                                .tooltip("Copy a LAYER_/MASK_ constant for every layer")
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(rust.clone()))
                                }),
                        )
                        .child(
                            Button::new("masks-copy-table")
                                .label("Copy table")
                                .ghost()
                                .small()
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(table.clone()))
                                }),
                        )
                        .into_any_element(),
                ),
                cx,
            ))
            .child(if n == 0 {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(empty_message(
                        IconName::Table,
                        "No layers",
                        "Add layers to see their masks",
                        cx,
                    ))
                    .into_any_element()
            } else {
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        h_flex()
                            .w_full()
                            .flex_shrink_0()
                            .px_3()
                            .py_1()
                            .gap_2()
                            .border_b_1()
                            .border_color(theme.border.opacity(0.6))
                            .child(header("BIT", 30.0))
                            .child(header("LAYER", 170.0))
                            .child(header("GROUP", 96.0))
                            .child(header("COLLIDES WITH", 96.0))
                            .child(header("PARTNERS", 120.0)),
                    )
                    .child(
                        v_flex()
                            .id("masks-scroll")
                            .flex_1()
                            .min_h_0()
                            .scrollable(ScrollbarAxis::Vertical)
                            .py_1()
                            .children(rows),
                    )
                    .into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{fix_all, fixes_for};
    use crate::assets::layers::{CollisionLayers, LayerFix, PRESETS};
    use crate::assets::Severity;

    #[test]
    fn a_one_sided_pair_offers_both_resolutions() {
        let mut set = CollisionLayers::default();
        set.add(Some("A"));
        set.collide_none();
        set.layers[1].collides_with.push("Default".into());
        let issue = set
            .validate()
            .into_iter()
            .find(|i| i.message.contains("doesn't list it back"))
            .unwrap();
        let fixes = fixes_for(&issue);
        assert_eq!(fixes.len(), 2);
        assert!(matches!(
            &fixes[0].1,
            LayerFix::SymmetrizePair { collide: true, .. }
        ));
        assert!(matches!(
            &fixes[1].1,
            LayerFix::SymmetrizePair { collide: false, .. }
        ));
        for (_, fix) in fixes {
            let mut copy = set.clone();
            copy.apply_fix(&fix);
            assert!(copy
                .validate()
                .iter()
                .all(|i| !i.message.contains("doesn't list it back")));
        }
    }

    #[test]
    fn fix_all_clears_everything_fixable_at_once() {
        let text = r#"{ "layers": [
            { "name": "A", "collides_with": ["A", "Ghost", "B"] },
            { "name": "B", "collides_with": [] },
            { "name": "a", "collides_with": [] } ] }"#;
        let (mut set, _) = <CollisionLayers as crate::assets::Asset>::parse(text).unwrap();
        assert!(set.validate().iter().any(|i| i.severity == Severity::Error));
        fix_all(&mut set);
        assert!(
            set.validate().iter().all(|i| i.severity == Severity::Info),
            "{:?}",
            set.validate()
        );
        // Presets need no fixing at all.
        let mut preset = PRESETS[1].build();
        let before = preset.clone();
        fix_all(&mut preset);
        assert_eq!(preset, before);
    }
}
