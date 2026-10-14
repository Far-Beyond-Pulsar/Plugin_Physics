//! The skeletal physics asset editor (`.physasset`).

use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::*;
use plugin_editor_api::{Redo, Undo, UndoService};
use skeletal_assets::ids::SkeletalMeshId;
use skeletal_assets::skeleton::Skeleton;
use ui::button::{Button, ButtonVariants};
use ui::input::{InputEvent, InputState, TextInput};
use ui::scroll::ScrollbarAxis;
use ui::tag::Tag;
use ui::{h_flex, v_flex, ActiveTheme, Icon, IconName, Sizable, StyledExt};

use crate::assets::rig::{load_skeleton_for_mesh, physics_assets_for_mesh, validate};
use crate::assets::Severity;
use crate::editors::common::document::Document;
use crate::editors::common::toolbar::{document_controls, file_banners, status_tag};
use pulsar_physics::{SkeletalJointKind, SkeletalMotion, SkeletalPhysicsAsset, SkeletalShape};

pub const EDITOR_ID: &str = "skeletal-physics-editor";

type Doc = Entity<Document<SkeletalPhysicsAsset>>;

pub struct RigEditor {
    doc: Doc,
    mesh_id: Entity<InputState>,
    skeleton: Option<Skeleton>,
    load_error: Option<String>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl RigEditor {
    pub fn new(
        path: PathBuf,
        undo: Option<&UndoService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc = cx.new(|_| {
            let mut doc = Document::<SkeletalPhysicsAsset>::open(Some(path));
            if let Some(undo) = undo {
                doc.adopt_undo(undo);
            }
            doc
        });
        let mesh_id = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Skeletal mesh asset ID")
                .default_value(doc.read(cx).asset().skeletal_mesh.to_string())
        });
        let d = doc.clone();
        let id_subscription = cx.subscribe_in(
            &mesh_id,
            window,
            move |_this, input, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = input.read(cx).value().trim().to_string();
                let Some(id) = SkeletalMeshId::parse(&text) else {
                    return;
                };
                if d.read(cx).asset().skeletal_mesh != id {
                    d.update(cx, |doc, cx| {
                        doc.edit("Change skeletal mesh", Some("skeletal-mesh"), cx, |asset| {
                            asset.skeletal_mesh = id;
                            asset.bodies.clear();
                            asset.constraints.clear();
                        });
                    });
                }
            },
        );
        let mut this = Self {
            doc,
            mesh_id,
            skeleton: None,
            load_error: None,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![id_subscription],
        };
        this.reload_skeleton(cx);
        this
    }

    fn reload_skeleton(&mut self, cx: &App) {
        let path = self.doc.read(cx).path().map(PathBuf::from);
        let Some(path) = path else {
            self.load_error = Some("Save the physics asset inside the project first".into());
            self.skeleton = None;
            return;
        };
        let mesh_id = self.doc.read(cx).asset().skeletal_mesh.clone();
        match load_skeleton_for_mesh(&path, &mesh_id) {
            Ok((_, skeleton)) => {
                self.skeleton = Some(skeleton.skeleton());
                self.load_error = duplicate_asset_diagnostic(&path, &mesh_id);
            }
            Err(error) => {
                self.skeleton = None;
                self.load_error = Some(error);
            }
        }
    }

    fn generate(&mut self, cx: &mut Context<Self>) {
        let path = self.doc.read(cx).path().map(PathBuf::from);
        let Some(path) = path else {
            self.load_error = Some("Save the physics asset inside the project first".into());
            cx.notify();
            return;
        };
        let mesh_id = self.doc.read(cx).asset().skeletal_mesh.clone();
        match load_skeleton_for_mesh(&path, &mesh_id) {
            Ok((_, skeleton)) => {
                let skeleton = skeleton.skeleton();
                let id = mesh_id.clone();
                self.doc.update(cx, |doc, cx| {
                    doc.edit("Generate bone bodies and joints", None, cx, |asset| {
                        let extra = std::mem::take(&mut asset.extra);
                        *asset = SkeletalPhysicsAsset::from_skeleton(id, &skeleton);
                        asset.extra = extra;
                    });
                });
                self.skeleton = Some(skeleton);
                self.load_error = duplicate_asset_diagnostic(&path, &mesh_id);
            }
            Err(error) => self.load_error = Some(error),
        }
        cx.notify();
    }

    fn cycle_shape(&self, bone_id: String, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| {
            doc.edit("Change collider shape", None, cx, |asset| {
                if let Some(body) = asset.bodies.iter_mut().find(|body| body.bone_id == bone_id) {
                    body.shape = match body.shape {
                        SkeletalShape::Box => SkeletalShape::Sphere,
                        SkeletalShape::Sphere => SkeletalShape::Capsule,
                        SkeletalShape::Capsule => SkeletalShape::Cylinder,
                        SkeletalShape::Cylinder => SkeletalShape::Box,
                    };
                }
            });
        });
    }

    fn cycle_motion(&self, bone_id: String, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| {
            doc.edit("Change body motion", None, cx, |asset| {
                if let Some(body) = asset.bodies.iter_mut().find(|body| body.bone_id == bone_id) {
                    body.motion = match body.motion {
                        SkeletalMotion::Dynamic => SkeletalMotion::Kinematic,
                        SkeletalMotion::Kinematic => SkeletalMotion::Fixed,
                        SkeletalMotion::Fixed => SkeletalMotion::Dynamic,
                    };
                }
            });
        });
    }

    fn scale_shape(&self, bone_id: String, scale: f32, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| {
            doc.edit("Resize collider", None, cx, |asset| {
                if let Some(body) = asset.bodies.iter_mut().find(|body| body.bone_id == bone_id) {
                    body.dimensions = body
                        .dimensions
                        .map(|value| (value * scale).clamp(0.01, 100.0));
                }
            });
        });
    }

    fn cycle_joint_kind(&self, bone_id: String, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| {
            doc.edit("Change joint type", None, cx, |asset| {
                if let Some(joint) = asset
                    .constraints
                    .iter_mut()
                    .find(|joint| joint.bone_id == bone_id)
                {
                    joint.kind = match joint.kind {
                        SkeletalJointKind::Fixed => SkeletalJointKind::Hinge,
                        SkeletalJointKind::Hinge => SkeletalJointKind::BallSocket,
                        SkeletalJointKind::BallSocket => SkeletalJointKind::Fixed,
                    };
                }
            });
        });
    }

    fn toggle_limits(&self, bone_id: String, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| {
            doc.edit("Toggle joint limits", None, cx, |asset| {
                if let Some(joint) = asset
                    .constraints
                    .iter_mut()
                    .find(|joint| joint.bone_id == bone_id)
                {
                    joint.limits_enabled = !joint.limits_enabled;
                }
            });
        });
    }

    fn adjust_limit(&self, bone_id: String, index: usize, amount: f32, cx: &mut Context<Self>) {
        self.doc.update(cx, |doc, cx| {
            doc.edit("Adjust joint limits", None, cx, |asset| {
                if let Some(joint) = asset
                    .constraints
                    .iter_mut()
                    .find(|joint| joint.bone_id == bone_id)
                {
                    if let Some(limit) = joint.limits_degrees.get_mut(index) {
                        *limit = (*limit + amount).clamp(-180.0, 180.0);
                    }
                }
            });
        });
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let doc = self.doc.read(cx);
        let asset = doc.asset();
        let issues = validate(asset);
        let worst = issues.iter().map(|issue| issue.severity).max();
        let body_count = asset.bodies.len();
        let constraint_count = asset.constraints.len();
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
            .child(document_controls("physasset", &self.doc, cx))
            .child(
                Icon::new(IconName::Box)
                    .size(px(15.0))
                    .text_color(theme.warning),
            )
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(doc.file_name()),
            )
            .child(
                Tag::secondary()
                    .small()
                    .child(format!("{body_count} bodies")),
            )
            .child(
                Tag::secondary()
                    .small()
                    .child(format!("{constraint_count} joints")),
            )
            .child(div().flex_1())
            .children(worst.map(|severity| {
                match severity {
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

    fn render_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let asset = self.doc.read(cx).asset().clone();
        let constraints = asset.constraints.clone();
        let rows = asset.bodies.iter().map(|body| {
            let bone_id = body.bone_id.clone();
            let shape_doc = bone_id.clone();
            let motion_doc = bone_id.clone();
            let smaller_doc = bone_id.clone();
            let larger_doc = bone_id.clone();
            h_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border.opacity(0.45))
                .child(div().flex_1().text_sm().child(body.bone_id.clone()))
                .child(
                    Button::new(SharedString::from(format!("shape-{}", body.bone_id)))
                        .label(format!("{:?}", body.shape))
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cycle_shape(shape_doc.clone(), cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "shape-smaller-{}",
                        body.bone_id
                    )))
                    .label("−")
                    .ghost()
                    .xsmall()
                    .tooltip("Shrink collider")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.scale_shape(smaller_doc.clone(), 0.9, cx)
                    })),
                )
                .child(
                    Button::new(SharedString::from(format!("shape-larger-{}", body.bone_id)))
                        .label("+")
                        .ghost()
                        .xsmall()
                        .tooltip("Grow collider")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.scale_shape(larger_doc.clone(), 1.1, cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("motion-{}", body.bone_id)))
                        .label(format!("{:?}", body.motion))
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cycle_motion(motion_doc.clone(), cx)
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!(
                            "{} joints",
                            asset
                                .constraints
                                .iter()
                                .filter(|joint| joint.bone_id == bone_id)
                                .count()
                        )),
                )
        });
        let joint_rows = constraints.iter().map(|joint| {
            let bone_id = joint.bone_id.clone();
            let kind_id = bone_id.clone();
            let limit_id = bone_id.clone();
            let lower_id = bone_id.clone();
            let upper_id = bone_id.clone();
            h_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border.opacity(0.45))
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .child(format!("{} → {}", joint.bone_id, joint.connected_bone_id)),
                )
                .child(
                    Button::new(SharedString::from(format!("joint-kind-{}", joint.bone_id)))
                        .label(format!("{:?}", joint.kind))
                        .ghost()
                        .xsmall()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cycle_joint_kind(kind_id.clone(), cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "joint-limits-{}",
                        joint.bone_id
                    )))
                    .label(if joint.limits_enabled {
                        format!(
                            "Limits {:.0}°..{:.0}°",
                            joint.limits_degrees[0], joint.limits_degrees[1]
                        )
                    } else {
                        "Limits off".to_string()
                    })
                    .ghost()
                    .xsmall()
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_limits(limit_id.clone(), cx)),
                    ),
                )
                .when(joint.limits_enabled, |row| {
                    row.child(
                        Button::new(SharedString::from(format!(
                            "joint-limit-min-{}",
                            joint.bone_id
                        )))
                        .label("−5°")
                        .ghost()
                        .xsmall()
                        .tooltip("Lower minimum limit")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.adjust_limit(lower_id.clone(), 0, -5.0, cx)
                        })),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "joint-limit-max-{}",
                            joint.bone_id
                        )))
                        .label("+5°")
                        .ghost()
                        .xsmall()
                        .tooltip("Raise maximum limit")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.adjust_limit(upper_id.clone(), 1, 5.0, cx)
                        })),
                    )
                })
        });
        v_flex()
            .gap_0()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .child("BONE BODIES"),
            )
            .children(rows)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .child("JOINTS AND LIMITS"),
            )
            .children(joint_rows)
    }

    fn render_skeleton_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let skeleton = self.skeleton.clone();
        let bodies = self.doc.read(cx).asset().bodies.clone();
        let status = self.load_error.clone();
        let overlay_theme = theme.clone();
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child("Bind pose collider overlay"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("physasset-generate")
                            .label("Generate from skeleton")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.generate(cx))),
                    )
                    .child(
                        Button::new("physasset-reload")
                            .label("Reload")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reload_skeleton(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(340.0))
                    .child(
                        gpui::canvas(
                            |bounds, _window, _cx| bounds,
                            move |_, bounds, window, _cx| {
                                let left = bounds.origin.x + px(25.0);
                                let top = bounds.origin.y + px(20.0);
                                let width = f32::from(bounds.size.width).max(20.0) - 50.0;
                                let height = f32::from(bounds.size.height).max(20.0) - 40.0;
                                let Some(skeleton) = skeleton.as_ref() else {
                                    return;
                                };
                                if skeleton.bones.is_empty() {
                                    return;
                                }
                                let positions = overlay_positions(skeleton, width, height);
                                let point_for = |id: &str| {
                                    positions
                                        .get(id)
                                        .map(|(x, y)| point(left + px(*x), top + px(*y)))
                                };
                                for bone in &skeleton.bones {
                                    if let Some(parent_id) = bone.parent.as_deref() {
                                        if let (Some(a), Some(b)) =
                                            (point_for(parent_id), point_for(&bone.id))
                                        {
                                            crate::editors::common::paint::stroke_polyline(
                                                window,
                                                &[a, b],
                                                2.0,
                                                overlay_theme.muted_foreground.opacity(0.8),
                                            );
                                        }
                                    }
                                }
                                for bone in &skeleton.bones {
                                    if let Some(p) = point_for(&bone.id) {
                                        let body =
                                            bodies.iter().find(|body| body.bone_id == bone.id);
                                        let color = if body.is_some() {
                                            overlay_theme.warning
                                        } else {
                                            overlay_theme.info.opacity(0.5)
                                        };
                                        let radius = body.map_or(4.0, |body| {
                                            (body.dimensions[0] * 16.0).clamp(5.0, 13.0)
                                        });
                                        crate::editors::common::paint::dot(
                                            window, p, radius, color,
                                        );
                                    }
                                }
                            },
                        )
                        .size_full(),
                    )
                    .children(
                        status
                            .map(|error| {
                                vec![div()
                                    .absolute()
                                    .left(px(10.0))
                                    .bottom(px(10.0))
                                    .max_w(px(520.0))
                                    .p_2()
                                    .rounded(px(6.0))
                                    .bg(theme.background)
                                    .text_xs()
                                    .text_color(theme.warning)
                                    .child(error)
                                    .into_any_element()]
                            })
                            .unwrap_or_default(),
                    ),
            )
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Mesh ID"),
                    )
                    .child(
                        div()
                            .w(px(420.0))
                            .child(TextInput::new(&self.mesh_id).small()),
                    )
                    .child(div().flex_1())
                    .child(Tag::warning().small().child("Schematic bind pose view")),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .scrollable(ScrollbarAxis::Vertical)
                    .child(self.render_rows(cx)),
            )
    }
}

fn duplicate_asset_diagnostic(path: &std::path::Path, mesh_id: &SkeletalMeshId) -> Option<String> {
    match physics_assets_for_mesh(path, mesh_id) {
        Ok(paths) if paths.len() > 1 => Some(format!(
            "Multiple .physasset files reference this mesh; runtime skips this rig: {}",
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Ok(_) => None,
        Err(error) => Some(format!(
            "Could not check for duplicate .physasset files: {error}"
        )),
    }
}

impl EventEmitter<ui::dock::PanelEvent> for RigEditor {}

impl Focusable for RigEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ui::dock::Panel for RigEditor {
    fn panel_name(&self) -> &'static str {
        EDITOR_ID
    }

    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.doc.read(cx).file_name().into())
    }

    fn tab_icon(&self, _cx: &App) -> Option<IconName> {
        Some(IconName::Box)
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

impl Render for RigEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let toolbar = self.render_toolbar(cx);
        let banners = file_banners("physasset", &self.doc, cx);
        div()
            .id("skeletal-physics-editor")
            .key_context("SkeletalPhysicsEditor")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .on_action(
                cx.listener(|this, _: &Undo, _, cx| this.doc.update(cx, |doc, cx| doc.undo(cx))),
            )
            .on_action(
                cx.listener(|this, _: &Redo, _, cx| this.doc.update(cx, |doc, cx| doc.redo(cx))),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.secondary() && event.keystroke.key == "s" {
                    this.doc.update(cx, |doc, cx| doc.save(cx));
                    cx.stop_propagation();
                }
            }))
            .child(toolbar)
            .children(banners)
            .child(self.render_skeleton_overlay(cx))
    }
}

fn overlay_positions(
    skeleton: &Skeleton,
    width: f32,
    height: f32,
) -> std::collections::HashMap<String, (f32, f32)> {
    let mut positions = std::collections::HashMap::new();
    let roots = skeleton.root_bones();
    let mut next_root_x = 0.0;
    fn place(
        skeleton: &Skeleton,
        bone_id: &str,
        x: f32,
        y: f32,
        depth: usize,
        positions: &mut std::collections::HashMap<String, (f32, f32)>,
    ) {
        positions.insert(bone_id.to_string(), (x, y));
        let children = skeleton.children_of(bone_id);
        if children.is_empty() {
            return;
        }
        let spread = (children.len() as f32 - 1.0) * 14.0;
        for (index, child) in children.into_iter().enumerate() {
            let offset_x = index as f32 * 28.0 - spread;
            let bind = child.bind_transform.translation;
            let local_x = (bind.x * 70.0).clamp(-45.0, 45.0) + offset_x;
            let local_y = (bind.y.abs() * 70.0).clamp(28.0, 80.0);
            place(
                skeleton,
                &child.id,
                x + local_x,
                y + local_y,
                depth + 1,
                positions,
            );
        }
        let _ = depth;
    }
    for root in roots {
        place(skeleton, &root.id, next_root_x, 0.0, 0, &mut positions);
        next_root_x += 100.0;
    }
    if positions.is_empty() {
        return positions;
    }
    let min_x = positions
        .values()
        .map(|p| p.0)
        .fold(f32::INFINITY, f32::min);
    let max_x = positions
        .values()
        .map(|p| p.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = positions
        .values()
        .map(|p| p.1)
        .fold(f32::INFINITY, f32::min);
    let max_y = positions
        .values()
        .map(|p| p.1)
        .fold(f32::NEG_INFINITY, f32::max);
    let scale = (width / (max_x - min_x).max(100.0))
        .min(height / (max_y - min_y).max(100.0))
        .min(1.0);
    positions
        .into_iter()
        .map(|(id, (x, y))| {
            (
                id,
                (
                    (x - min_x) * scale + (width - (max_x - min_x) * scale) * 0.5,
                    (y - min_y) * scale + 12.0,
                ),
            )
        })
        .collect()
}
