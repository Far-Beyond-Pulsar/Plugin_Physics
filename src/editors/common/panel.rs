//! Boilerplate shared by the dock panels of both editors.

use std::sync::Arc;

use gpui::*;
use ui::dock::{DockChannel, DockItem, PanelView};
use ui::workspace::Workspace;

/// Implement the dock traits for a panel struct that has a `focus_handle`
/// field: `dock_panel!(MyPanel, "unique-name", "Title");`.
///
/// `panel_name` is how a saved layout identifies the panel, so it must be
/// unique and never change.
macro_rules! dock_panel {
    ($ty:ty, $name:expr, $title:expr) => {
        impl gpui::EventEmitter<ui::dock::PanelEvent> for $ty {}

        impl gpui::Focusable for $ty {
            fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
                self.focus_handle.clone()
            }
        }

        impl ui::dock::Panel for $ty {
            fn panel_name(&self) -> &'static str {
                $name
            }

            fn title(&self, _window: &gpui::Window, _cx: &gpui::App) -> gpui::AnyElement {
                gpui::IntoElement::into_any_element($title)
            }
        }
    };
}
pub(crate) use dock_panel;

/// One dock: its tabs and its size (width, or height for the bottom dock).
pub struct Dock {
    pub panels: Vec<Arc<dyn PanelView>>,
    pub size: f32,
}

impl Dock {
    pub fn new(size: f32, panels: Vec<Arc<dyn PanelView>>) -> Self {
        Self { panels, size }
    }
}

/// Where an editor's panels go.
pub struct Layout {
    pub center: Vec<Arc<dyn PanelView>>,
    pub left: Option<Dock>,
    pub right: Option<Dock>,
    pub bottom: Option<Dock>,
}

/// A workspace laid out as `layout`. `channel` keeps panels of different
/// editors from being dragged into each other.
pub fn build_workspace<V: 'static>(
    name: &'static str,
    channel: u32,
    layout: Layout,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Entity<Workspace> {
    let workspace = cx.new(|cx| Workspace::new_with_channel(name, DockChannel(channel), window, cx));
    workspace.update(cx, |workspace, cx| {
        let area_weak = workspace.dock_area().downgrade();
        let tabs = |panels: Vec<Arc<dyn PanelView>>, window: &mut Window, cx: &mut Context<Workspace>| {
            DockItem::tabs(panels, Some(0), &area_weak, window, cx)
        };

        let center = tabs(layout.center, window, cx);
        let left = layout.left.map(|d| (tabs(d.panels, window, cx), d.size));
        let right = layout.right.map(|d| (tabs(d.panels, window, cx), d.size));
        let bottom = layout.bottom.map(|d| (tabs(d.panels, window, cx), d.size));

        workspace.dock_area().update(cx, |area, cx| {
            area.set_center(center, window, cx);
            if let Some((item, size)) = left {
                area.set_left_dock(item, Some(px(size)), true, window, cx);
            }
            if let Some((item, size)) = right {
                area.set_right_dock(item, Some(px(size)), true, window, cx);
            }
            if let Some((item, size)) = bottom {
                area.set_bottom_dock(item, Some(px(size)), true, window, cx);
            }
        });
    });
    workspace
}
