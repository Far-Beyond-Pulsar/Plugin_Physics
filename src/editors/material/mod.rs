//! The Physics Material editor (`.physmat`).
//!
//! Layout: presets on the left, live bounce and slope previews in the middle,
//! the material's properties on the right, and how it interacts with other
//! materials and what it weighs along the bottom. All panels share one
//! [`Document`], which owns the file, the undo history and the dirty state.

mod charts;
mod interaction;
mod preview;
mod presets;
mod properties;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use plugin_editor_api::{Redo, Undo, UndoService};
use ui::dock::{PanelEvent, PanelView};
use ui::tag::Tag;
use ui::workspace::Workspace;
use ui::{h_flex, ActiveTheme, Icon, IconName, Sizable, StyledExt};

use crate::assets::material::PhysicsMaterial;
use crate::assets::Severity;
use crate::editors::common::chrome::{color_of, swatch, toolbar_divider};
use crate::editors::common::document::Document;
use crate::editors::common::panel::{build_workspace, Dock, Layout};
use crate::editors::common::toolbar::{document_controls, file_banners, status_tag};

use interaction::{InteractionPanel, MassPanel};
use preview::PreviewPanel;
use presets::PresetsPanel;
use properties::MaterialProperties;

pub const EDITOR_ID: &str = "physics-material-editor";

/// Dock channel: panels only move between docks of the same editor kind.
const DOCK_CHANNEL: u32 = 7;

fn as_panel<P: PanelView + 'static>(panel: P) -> Arc<dyn PanelView> {
    Arc::new(panel)
}

pub struct MaterialEditor {
    doc: Entity<Document<PhysicsMaterial>>,
    workspace: Entity<Workspace>,
    focus_handle: FocusHandle,
}

impl MaterialEditor {
    pub fn new(
        path: PathBuf,
        undo: Option<&UndoService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc = cx.new(|_| {
            let mut doc = Document::<PhysicsMaterial>::open(Some(path));
            if let Some(undo) = undo {
                doc.adopt_undo(undo);
            }
            doc
        });

        let presets = cx.new(|cx| PresetsPanel::new(doc.clone(), window, cx));
        let preview = cx.new(|cx| PreviewPanel::new(doc.clone(), window, cx));
        let properties = cx.new(|cx| MaterialProperties::new(doc.clone(), window, cx));
        let interactions = cx.new(|cx| InteractionPanel::new(doc.clone(), window, cx));
        let mass = cx.new(|cx| MassPanel::new(doc.clone(), window, cx));

        let layout = Layout {
            center: vec![as_panel(preview)],
            left: Some(Dock::new(270.0, vec![as_panel(presets)])),
            right: Some(Dock::new(350.0, vec![as_panel(properties)])),
            bottom: Some(Dock::new(250.0, vec![as_panel(interactions), as_panel(mass)])),
        };
        let workspace = build_workspace("physics-material-workspace", DOCK_CHANNEL, layout, window, cx);

        Self { doc, workspace, focus_handle: cx.focus_handle() }
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (name, surface_color, surface_label, issue_count, worst) = {
            let doc = self.doc.read(cx);
            let m = doc.asset();
            let issues = m.validate();
            (
                doc.file_name(),
                color_of(m.surface.tint()),
                m.surface.label(),
                issues.len(),
                issues.iter().map(|i| i.severity).max(),
            )
        };

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
            .child(document_controls("material", &self.doc, cx))
            .child(toolbar_divider(cx))
            .child(Icon::new(IconName::Droplet).size(px(15.0)).text_color(theme.info))
            .child(div().text_sm().font_semibold().text_color(theme.foreground).child(name))
            .child(swatch(surface_color, 14.0, cx))
            .child(Tag::secondary().small().child(surface_label))
            .child(div().flex_1())
            .children(worst.map(|w| match w {
                Severity::Error => Tag::danger().small().child(format!("{issue_count} issues")).into_any_element(),
                Severity::Warning => Tag::warning().small().child(format!("{issue_count} issues")).into_any_element(),
                Severity::Info => Tag::info().small().child(format!("{issue_count} notes")).into_any_element(),
            }))
            .child(status_tag(&self.doc, cx))
    }

}

impl EventEmitter<PanelEvent> for MaterialEditor {}

impl Focusable for MaterialEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ui::dock::Panel for MaterialEditor {
    fn panel_name(&self) -> &'static str {
        EDITOR_ID
    }

    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.doc.read(cx).file_name().into())
    }

    fn tab_icon(&self, _cx: &App) -> Option<IconName> {
        Some(IconName::Droplet)
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

impl Render for MaterialEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let toolbar = self.render_toolbar(cx);
        let banners = file_banners("material", &self.doc, cx);

        div()
            .id("physics-material-editor")
            .key_context("PhysicsMaterialEditor")
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
