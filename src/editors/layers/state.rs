//! State the layers editor's panels share beyond the document: which layer
//! is selected, and the operations that change the layer list while keeping
//! that selection pointing at the right layer.

use gpui::*;

use crate::assets::layers::CollisionLayers;
use crate::editors::common::document::Document;

pub type Doc = Entity<Document<CollisionLayers>>;
pub type Sel = Entity<LayerSelection>;

/// Which layer the panels are looking at.
#[derive(Default)]
pub struct LayerSelection {
    selected: Option<usize>,
}

impl LayerSelection {
    /// The selected layer, if it still exists in a list of `len` layers.
    pub fn current(&self, len: usize) -> Option<usize> {
        self.selected.filter(|i| *i < len)
    }

    pub fn select(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if self.selected != index {
            self.selected = index;
            cx.notify();
        }
    }
}

/// Add a layer and select it. Does nothing at the 32-layer limit.
pub fn add_layer(doc: &Doc, sel: &Sel, cx: &mut App) {
    let mut added = None;
    doc.update(cx, |d, cx| {
        d.edit("Add layer", None, cx, |set| added = set.add(None))
    });
    if let Some(i) = added {
        sel.update(cx, |s, cx| s.select(Some(i), cx));
    }
}

/// Remove layer `i`, selecting its neighbour.
pub fn remove_layer(doc: &Doc, sel: &Sel, i: usize, cx: &mut App) {
    let mut left = 0;
    doc.update(cx, |d, cx| {
        d.edit("Delete layer", None, cx, |set| {
            set.remove(i);
            left = set.len();
        })
    });
    let next = if left == 0 {
        None
    } else {
        Some(i.min(left - 1))
    };
    sel.update(cx, |s, cx| s.select(next, cx));
}

/// Duplicate layer `i` and select the copy.
pub fn duplicate_layer(doc: &Doc, sel: &Sel, i: usize, cx: &mut App) {
    let mut copy = None;
    doc.update(cx, |d, cx| {
        d.edit("Duplicate layer", None, cx, |set| copy = set.duplicate(i))
    });
    if let Some(c) = copy {
        sel.update(cx, |s, cx| s.select(Some(c), cx));
    }
}

/// Move layer `i` by `delta` places and keep it selected.
pub fn move_layer(doc: &Doc, sel: &Sel, i: usize, delta: isize, cx: &mut App) {
    let len = doc.read(cx).asset().len() as isize;
    let target = (i as isize + delta).clamp(0, len - 1) as usize;
    if target == i {
        return;
    }
    doc.update(cx, |d, cx| {
        d.edit("Reorder layers", None, cx, |set| set.move_layer(i, target))
    });
    sel.update(cx, |s, cx| s.select(Some(target), cx));
}
