//! Renders the real editors headlessly and drives them.
//!
//! Each test builds an editor in a test window, draws it, makes edits through
//! the shared document (as the panels' controls do), and draws again after
//! every step. That runs every panel's `render` against real state, which is
//! where re-entrant entity borrows and bad indices would show up.

use std::path::PathBuf;

use gpui::{TestAppContext, VisualTestContext};
use ui::dock::Panel;

use crate::assets::layers::CollisionLayers;
use crate::assets::material::{PhysicsMaterial, PRESETS};
use crate::editors::common::document::Status;
use crate::editors::layers::LayersEditor;
use crate::editors::material::MaterialEditor;

fn temp_file(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("physics_editor_ui_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

/// A few frames: docks size themselves over the first couple.
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        // Let timed UI work finish (a checkbox's toggle animation holds its
        // state until its timer fires), so nothing is pending when the test
        // ends.
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn the_material_editor_renders_edits_undoes_and_saves(cx: &mut TestAppContext) {
    cx.update(ui::init);
    let path = temp_file(
        "rubber.physmat",
        r#"{ "friction": 0.9, "surface": "rubber" }"#,
    );
    let (editor, cx) =
        cx.add_window_view(|window, cx| MaterialEditor::new(path.clone(), None, window, cx));
    draw(cx);

    let doc = editor.update(cx, |e, _| e.doc.clone());
    let unsaved = |editor: &gpui::Entity<MaterialEditor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, cx| e.tab_unsaved(cx))
    };
    assert_eq!(
        editor.read_with(cx, |e, cx| e.tab_name(cx)).as_deref(),
        Some("rubber")
    );
    assert_eq!(
        editor.read_with(cx, |e, cx| e.panel_file_path(cx)),
        Some(path.clone())
    );
    assert!(!unsaved(&editor, cx), "a freshly opened file is clean");

    // Edit through the document, the way the controls do.
    let ice = PRESETS.iter().find(|p| p.name == "Ice").unwrap();
    doc.update(cx, |d, cx| {
        d.edit("Apply preset", None, cx, |m| {
            m.friction = ice.friction;
            m.restitution = ice.restitution;
        })
    });
    draw(cx);
    assert!(unsaved(&editor, cx));
    assert_eq!(doc.read_with(cx, |d, _| d.asset().friction), 0.03);

    // A burst of slider-style edits is one undo step.
    for i in 1..=6 {
        doc.update(cx, |d, cx| {
            d.edit("Change friction", Some("friction"), cx, |m| {
                m.friction = 0.1 * i as f32
            })
        });
        draw(cx);
    }
    doc.update(cx, |d, cx| d.undo(cx));
    draw(cx);
    assert_eq!(
        doc.read_with(cx, |d, _| d.asset().friction),
        0.03,
        "one undo reverts the whole drag"
    );
    doc.update(cx, |d, cx| d.redo(cx));
    draw(cx);
    assert!((doc.read_with(cx, |d, _| d.asset().friction) - 0.6).abs() < 1e-5);

    doc.update(cx, |d, cx| d.undo(cx));
    doc.update(cx, |d, cx| d.undo(cx));
    draw(cx);
    assert!(
        !unsaved(&editor, cx),
        "undoing everything returns to the saved state"
    );
    assert_eq!(doc.read_with(cx, |d, _| d.asset().friction), 0.9);

    // Save and read it back.
    doc.update(cx, |d, cx| {
        d.edit("Change density", None, cx, |m| m.density = 2500.0)
    });
    doc.update(cx, |d, cx| d.save(cx));
    draw(cx);
    assert!(!unsaved(&editor, cx));
    assert_eq!(
        doc.read_with(cx, |d, _| d.status().cloned()),
        Some(Status::Saved)
    );
    let (saved, _) =
        <PhysicsMaterial as crate::assets::Asset>::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap();
    assert_eq!(saved.density, 2500.0);
    assert_eq!(saved.friction, 0.9);

    // Out-of-range values render with their validation messages.
    doc.update(cx, |d, cx| {
        d.edit("Break it", None, cx, |m| {
            m.friction = -2.0;
            m.restitution = 3.0;
            m.density = 0.0;
        })
    });
    draw(cx);
    // The edit keeps friction at 0 or more and restitution at 2 or less; what
    // is left out of range shows its messages.
    let messages = doc.read_with(cx, |d, _| {
        d.asset()
            .validate()
            .into_iter()
            .map(|issue| issue.message)
            .collect::<Vec<_>>()
    });
    assert!(
        messages.iter().any(|m| m.contains("adds energy")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Density must be greater than 0")),
        "{messages:?}"
    );
    doc.update(cx, |d, cx| d.revert(cx));
    draw(cx);
    assert_eq!(
        doc.read_with(cx, |d, _| d.asset().density),
        2500.0,
        "revert reloads the file"
    );
}

#[gpui::test]
fn an_unreadable_material_is_protected_until_the_user_starts_over(cx: &mut TestAppContext) {
    cx.update(ui::init);
    let path = temp_file("broken.physmat", "{ this is not json");
    let (editor, cx) =
        cx.add_window_view(|window, cx| MaterialEditor::new(path.clone(), None, window, cx));
    draw(cx);
    let doc = editor.update(cx, |e, _| e.doc.clone());

    assert!(doc.read_with(cx, |d, _| d.unreadable().is_some()));
    doc.update(cx, |d, cx| d.save(cx));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{ this is not json",
        "save must not clobber it"
    );

    doc.update(cx, |d, cx| d.start_fresh(cx));
    draw(cx);
    assert!(doc.read_with(cx, |d, _| d.unreadable().is_none()));
    assert!(editor.read_with(cx, |e, cx| e.tab_unsaved(cx)));
    doc.update(cx, |d, cx| d.save(cx));
    assert!(std::fs::read_to_string(&path).unwrap().contains("friction"));
}

#[gpui::test]
fn a_material_with_repaired_values_shows_what_was_changed(cx: &mut TestAppContext) {
    cx.update(ui::init);
    let path = temp_file(
        "odd.physmat",
        r#"{ "friction": "high", "friction_combine": "sum" }"#,
    );
    let (editor, cx) = cx.add_window_view(|window, cx| MaterialEditor::new(path, None, window, cx));
    draw(cx);
    let doc = editor.update(cx, |e, _| e.doc.clone());
    assert_eq!(doc.read_with(cx, |d, _| d.load_notes().len()), 2);
    doc.update(cx, |d, cx| d.dismiss_load_notes(cx));
    draw(cx);
    assert!(doc.read_with(cx, |d, _| d.load_notes().is_empty()));
}

#[gpui::test]
fn the_layers_editor_renders_and_every_edit_redraws_cleanly(cx: &mut TestAppContext) {
    cx.update(ui::init);
    let path = temp_file(
        "world.physlayers",
        r#"{ "layers": [ { "name": "Default", "collides_with": ["Default"] } ] }"#,
    );
    let (editor, cx) =
        cx.add_window_view(|window, cx| LayersEditor::new(path.clone(), None, window, cx));
    draw(cx);
    let doc = editor.update(cx, |e, _| e.doc.clone());
    let len = |doc: &gpui::Entity<_>, cx: &mut VisualTestContext| {
        doc.read_with(
            cx,
            |d: &crate::editors::common::document::Document<CollisionLayers>, _| d.asset().len(),
        )
    };
    assert_eq!(
        editor.read_with(cx, |e, cx| e.tab_name(cx)).as_deref(),
        Some("world")
    );

    // Grow the set, flip pairs, break a pairing, reorder, duplicate, remove.
    for name in ["Player", "Enemy", "Projectile", "Trigger"] {
        doc.update(cx, |d, cx| {
            d.edit("Add layer", None, cx, |s| drop(s.add(Some(name))))
        });
        draw(cx);
    }
    assert_eq!(len(&doc, cx), 5);
    doc.update(cx, |d, cx| {
        d.edit("Toggle", None, cx, |s| s.toggle_pair(1, 2))
    });
    draw(cx);
    doc.update(cx, |d, cx| {
        d.edit("Break a pair", None, cx, |s| {
            s.layers[3].collides_with.clear()
        })
    });
    draw(cx);
    assert!(doc.read_with(cx, |d, _| d
        .asset()
        .validate()
        .iter()
        .any(|i| i.message.contains("doesn't list it back"))));

    doc.update(cx, |d, cx| d.edit("Move", None, cx, |s| s.move_layer(4, 0)));
    doc.update(cx, |d, cx| {
        d.edit("Duplicate", None, cx, |s| drop(s.duplicate(2)))
    });
    draw(cx);
    assert_eq!(len(&doc, cx), 6);

    doc.update(cx, |d, cx| {
        d.edit("Fix", None, cx, |s| {
            for issue in s.validate() {
                if let Some(fix) = issue.fix {
                    s.apply_fix(&fix);
                }
            }
        })
    });
    draw(cx);

    // Undo all the way back, then redo: every intermediate state must render.
    for _ in 0..16 {
        doc.update(cx, |d, cx| d.undo(cx));
        draw(cx);
    }
    assert_eq!(len(&doc, cx), 1, "undo returns to the file as opened");
    assert!(!editor.read_with(cx, |e, cx| e.tab_unsaved(cx)));
    for _ in 0..16 {
        doc.update(cx, |d, cx| d.redo(cx));
        draw(cx);
    }
    assert_eq!(len(&doc, cx), 6);

    // The empty set has its own states in every panel.
    doc.update(cx, |d, cx| d.edit("Empty", None, cx, |s| s.layers.clear()));
    draw(cx);
    doc.update(cx, |d, cx| d.save(cx));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"layers\": []"), "{text}");
}

#[gpui::test]
fn a_one_sided_pair_and_a_full_set_render_with_their_warnings(cx: &mut TestAppContext) {
    cx.update(ui::init);
    let path = temp_file(
        "full.physlayers",
        r#"{ "layers": [ { "name": "Default", "collides_with": ["Default"] } ] }"#,
    );
    let (editor, cx) = cx.add_window_view(|window, cx| LayersEditor::new(path, None, window, cx));
    draw(cx);
    let doc = editor.update(cx, |e, _| e.doc.clone());

    // All 32 layers: the matrix, graph and masks at their largest.
    doc.update(cx, |d, cx| {
        d.edit("Fill", None, cx, |s| while s.add(None).is_some() {})
    });
    draw(cx);
    assert_eq!(doc.read_with(cx, |d, _| d.asset().len()), 32);
    doc.update(cx, |d, cx| {
        d.edit("One-sided", None, cx, |s| {
            s.layers[31].collides_with.clear();
            s.layers[0].collides_with.push("Layer 31".to_string());
        })
    });
    draw(cx);
}

#[gpui::test]
fn a_layers_file_with_problems_opens_and_reports_them(cx: &mut TestAppContext) {
    cx.update(ui::init);
    let path = temp_file(
        "messy.physlayers",
        r#"{ "layers": [ { "name": "A", "collides_with": ["A", "Ghost", "A"] }, { "name": "a" }, 7 ] }"#,
    );
    let (editor, cx) = cx.add_window_view(|window, cx| LayersEditor::new(path, None, window, cx));
    draw(cx);
    let doc = editor.update(cx, |e, _| e.doc.clone());
    assert!(
        !doc.read_with(cx, |d, _| d.load_notes().is_empty()),
        "the skipped entry is reported"
    );
    assert!(doc.read_with(cx, |d, _| d
        .asset()
        .validate()
        .iter()
        .any(|i| i.fix.is_some())));
}
