//! The Collision Layers editor (`.physlayers`).
//!
//! Layout: the layer list and presets on the left, the collision matrix and a
//! relationship graph in the middle, the selected layer's inspector on the
//! right, and the issues and the engine's masks along the bottom. All panels
//! share one [`Document`] (the file, undo history and dirty state) and one
//! selection.

mod graph;
mod inspector;
mod issues;
mod list;
mod matrix;
mod presets;
mod state;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use plugin_editor_api::{Redo, Undo, UndoService};
use ui::dock::{PanelEvent, PanelView};
use ui::tag::Tag;
use ui::workspace::Workspace;
use ui::{h_flex, ActiveTheme, Icon, IconName, Sizable, StyledExt};

use crate::assets::layers::{CollisionLayers, MAX_LAYERS};
use crate::assets::Severity;
use crate::editors::common::chrome::toolbar_divider;
use crate::editors::common::document::Document;
use crate::editors::common::panel::{build_workspace, Dock, Layout};
use crate::editors::common::toolbar::{document_controls, file_banners, status_tag};

use graph::LayerGraph;
use inspector::LayerInspector;
use issues::{IssuesPanel, MasksPanel};
use list::LayersList;
use matrix::CollisionMatrix;
use presets::LayerPresets;
use state::LayerSelection;

pub const EDITOR_ID: &str = "collision-layers-editor";

/// Dock channel: panels only move between docks of the same editor kind.
const DOCK_CHANNEL: u32 = 8;

fn as_panel<P: PanelView + 'static>(panel: P) -> Arc<dyn PanelView> {
    Arc::new(panel)
}

pub struct LayersEditor {
    pub(crate) doc: Entity<Document<CollisionLayers>>,
    workspace: Entity<Workspace>,
    focus_handle: FocusHandle,
}

impl LayersEditor {
    pub fn new(
        path: PathBuf,
        undo: Option<&UndoService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc = cx.new(|_| {
            let mut doc = Document::<CollisionLayers>::open(Some(path));
            if let Some(undo) = undo {
                doc.adopt_undo(undo);
            }
            doc
        });
        // Start with the first layer selected so the inspector isn't empty.
        let sel = cx.new(|_| LayerSelection::default());
        sel.update(cx, |s, cx| s.select(Some(0), cx));

        let list = cx.new(|cx| LayersList::new(doc.clone(), sel.clone(), window, cx));
        let presets = cx.new(|cx| LayerPresets::new(doc.clone(), sel.clone(), cx));
        let matrix = cx.new(|cx| CollisionMatrix::new(doc.clone(), sel.clone(), cx));
        let graph = cx.new(|cx| LayerGraph::new(doc.clone(), sel.clone(), cx));
        let inspector = cx.new(|cx| LayerInspector::new(doc.clone(), sel.clone(), window, cx));
        let issues = cx.new(|cx| IssuesPanel::new(doc.clone(), sel.clone(), cx));
        let masks = cx.new(|cx| MasksPanel::new(doc.clone(), cx));

        let layout = Layout {
            center: vec![as_panel(matrix), as_panel(graph)],
            left: Some(Dock::new(300.0, vec![as_panel(list), as_panel(presets)])),
            right: Some(Dock::new(340.0, vec![as_panel(inspector)])),
            bottom: Some(Dock::new(220.0, vec![as_panel(issues), as_panel(masks)])),
        };
        let workspace = build_workspace(
            "collision-layers-workspace",
            DOCK_CHANNEL,
            layout,
            window,
            cx,
        );

        Self {
            doc,
            workspace,
            focus_handle: cx.focus_handle(),
        }
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (name, layers, issues) = {
            let doc = self.doc.read(cx);
            (doc.file_name(), doc.asset().len(), doc.asset().validate())
        };
        let worst = issues.iter().map(|i| i.severity).max();

        h_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(40.0))
            .px_2()
            .gap_2()
            .items_center()
            .bg(theme.secondary)
            .border_b_1()
            .border_color(theme.border)
            .child(document_controls("layers", &self.doc, cx))
            .child(toolbar_divider(cx))
            .child(
                Icon::new(IconName::List)
                    .size(px(15.0))
                    .text_color(theme.info),
            )
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(name),
            )
            .child(if layers >= MAX_LAYERS {
                Tag::warning()
                    .small()
                    .child(format!("{layers} / {MAX_LAYERS} layers"))
            } else {
                Tag::secondary()
                    .small()
                    .child(format!("{layers} / {MAX_LAYERS} layers"))
            })
            .child(div().flex_1())
            .children(worst.map(|w| {
                match w {
                    Severity::Error => Tag::danger()
                        .small()
                        .child(format!("{} issues", issues.len()))
                        .into_any_element(),
                    Severity::Warning => Tag::warning()
                        .small()
                        .child(format!("{} issues", issues.len()))
                        .into_any_element(),
                    Severity::Info => Tag::info()
                        .small()
                        .child(format!("{} notes", issues.len()))
                        .into_any_element(),
                }
            }))
            .child(status_tag(&self.doc, cx))
    }
}

impl EventEmitter<PanelEvent> for LayersEditor {}

impl Focusable for LayersEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ui::dock::Panel for LayersEditor {
    fn panel_name(&self) -> &'static str {
        EDITOR_ID
    }

    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.doc.read(cx).file_name().into())
    }

    fn tab_icon(&self, _cx: &App) -> Option<IconName> {
        Some(IconName::List)
    }

    fn tab_unsaved(&self, cx: &App) -> bool {
        self.doc.read(cx).is_dirty()
    }

    fn panel_file_path(&self, cx: &App) -> Option<PathBuf> {
        self.doc.read(cx).path().map(PathBuf::from)
    }

    fn title(&self, _window: &Window, cx: &App) -> AnyElement {
        SharedString::from(self.doc.read(cx).file_name()).into_any_element()
    }
}

impl Render for LayersEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let toolbar = self.render_toolbar(cx);
        let banners = file_banners("layers", &self.doc, cx);

        div()
            .id("collision-layers-editor")
            .key_context("CollisionLayersEditor")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            // The host's shared Undo/Redo act on this file's history.
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.doc.update(cx, |d, cx| d.undo(cx))))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.doc.update(cx, |d, cx| d.redo(cx))))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.secondary() && event.keystroke.key == "s" {
                    this.doc.update(cx, |d, cx| d.save(cx));
                    cx.stop_propagation();
                }
            }))
            .child(toolbar)
            .children(banners)
            .child(div().flex_1().min_h_0().child(self.workspace.clone()))
    }
}
