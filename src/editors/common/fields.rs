//! Property fields shared by the editors.

use gpui::*;
use ui::input::{InputEvent, InputState, NumberInput, NumberInputEvent, StepAction};
use ui::slider::{Slider, SliderEvent, SliderState};
use ui::{h_flex, ActiveTheme, Disableable, Sizable};

use crate::assets::Asset;

use super::document::Document;

/// What a [`ScalarField`] edits and how.
pub struct ScalarSpec<A: Asset> {
    /// The undo label ("Change friction").
    pub label: &'static str,
    /// Edits with this key made in quick succession are one undo step.
    pub key: &'static str,
    pub min: f32,
    /// The slider's right end. The number box accepts values up to `hard_max`.
    pub max: f32,
    pub hard_max: f32,
    pub step: f32,
    pub decimals: usize,
    pub get: fn(&A) -> f32,
    pub set: fn(&mut A, f32),
}

// Fields are plain data, so `Clone` doesn't need `A: Clone`'s help.
impl<A: Asset> std::clone::Clone for ScalarSpec<A> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<A: Asset> std::marker::Copy for ScalarSpec<A> {}

/// A number edited by a slider and a typed field that stay in step, wired to
/// a document: dragging or typing edits it (undoably), and undo, redo and
/// presets move the controls.
pub struct ScalarField<A: Asset> {
    slider: Entity<SliderState>,
    input: Entity<InputState>,
    spec: ScalarSpec<A>,
    _subscriptions: Vec<Subscription>,
}

impl<A: Asset> ScalarField<A> {
    pub fn new<V: 'static>(
        doc: &Entity<Document<A>>,
        spec: ScalarSpec<A>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let initial = (spec.get)(doc.read(cx).asset());
        let slider = cx.new(|_| {
            SliderState::new()
                .min(spec.min)
                .max(spec.max)
                .step(spec.step)
                .default_value(initial.clamp(spec.min, spec.max))
        });
        let input = cx.new(|cx| {
            InputState::new(window, cx).default_value(format!("{initial:.*}", spec.decimals))
        });

        // One place that turns "the user wants `value`" into an edit, if the
        // document doesn't already have it.
        fn apply<A: Asset, V: 'static>(
            doc: &Entity<Document<A>>,
            spec: &ScalarSpec<A>,
            value: f32,
            cx: &mut Context<V>,
        ) {
            if !value.is_finite() {
                return;
            }
            let value = value.clamp(spec.min.min(0.0), spec.hard_max);
            if ((spec.get)(doc.read(cx).asset()) - value).abs() < 1e-6 {
                return;
            }
            let set = spec.set;
            doc.update(cx, |d, cx| {
                d.edit(spec.label, Some(spec.key), cx, |a| set(a, value))
            });
        }

        let mut subscriptions = Vec::new();

        let d = doc.clone();
        subscriptions.push(cx.subscribe_in(
            &slider,
            window,
            move |_this: &mut V, _slider, event: &SliderEvent, _window, cx| {
                let SliderEvent::Change(value) = event;
                apply(&d, &spec, value.end(), cx);
            },
        ));

        let d = doc.clone();
        subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |_this: &mut V, input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Ok(value) = input.read(cx).value().trim().parse::<f32>() {
                        apply(&d, &spec, value, cx);
                    }
                }
            },
        ));

        let d = doc.clone();
        subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |_this: &mut V, _input, event: &NumberInputEvent, _window, cx| {
                let NumberInputEvent::Step { action, fine } = event;
                let step = if *fine { spec.step * 0.1 } else { spec.step };
                let current = (spec.get)(d.read(cx).asset());
                let next = match action {
                    StepAction::Increment => current + step,
                    StepAction::Decrement => current - step,
                };
                apply(&d, &spec, next, cx);
            },
        ));

        Self {
            slider,
            input,
            spec,
            _subscriptions: subscriptions,
        }
    }

    /// Bring the controls to `value` (the document's). Cheap when they
    /// already show it; leaves the typed field alone while it has focus.
    pub fn sync<V: 'static>(&self, value: f32, window: &mut Window, cx: &mut Context<V>) {
        let target = value.clamp(self.spec.min, self.spec.max);
        if (self.slider.read(cx).value().end() - target).abs() > 1e-5 {
            self.slider
                .update(cx, |s, cx| s.set_value(target, window, cx));
        }
        let text = format!("{value:.*}", self.spec.decimals);
        let focused = self
            .input
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx);
        let current = self.input.read(cx).value().to_string();
        let same_number = current
            .trim()
            .parse::<f32>()
            .is_ok_and(|v| (v - value).abs() < 1e-6);
        if !focused && current != text && !same_number {
            self.input.update(cx, |i, cx| i.set_value(text, window, cx));
        }
    }

    /// The slider above the typed field.
    pub fn render(&self, disabled: bool, cx: &App) -> impl IntoElement {
        let _ = cx.theme();
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .child(Slider::new(&self.slider).disabled(disabled)),
            )
            .child(
                div()
                    .w(px(86.0))
                    .child(NumberInput::new(&self.input).small().disabled(disabled)),
            )
    }
}
