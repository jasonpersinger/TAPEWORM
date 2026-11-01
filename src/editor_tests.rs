use super::*;
use std::sync::Mutex;

#[derive(Debug, PartialEq)]
enum Event {
    Begin,
    Set(f32),
    End,
}

// Deliberately leave the parameter unchanged to model delayed host feedback.
#[derive(Default)]
struct RecordingHost(Mutex<Vec<Event>>);

impl GuiContext for RecordingHost {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    fn request_resize(&self) -> bool {
        true
    }

    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {
        self.0.lock().unwrap().push(Event::Begin);
    }

    unsafe fn raw_set_parameter_normalized(&self, _: ParamPtr, normalized: f32) {
        self.0.lock().unwrap().push(Event::Set(normalized));
    }

    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {
        self.0.lock().unwrap().push(Event::End);
    }

    fn get_state(&self) -> PluginState {
        unreachable!()
    }

    fn set_state(&self, _: PluginState) {
        unreachable!()
    }
}

fn frame<P: Param>(
    ctx: &egui::Context,
    param: &P,
    host: &RecordingHost,
    events: Vec<egui::Event>,
    shift: bool,
) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 200.0))),
            events,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw_deck_knob(
                    ui,
                    pos2(100.0, 100.0),
                    45.0,
                    28.0,
                    6.0,
                    1.0,
                    param,
                    &ParamSetter::new(host),
                );
            });
        },
    );
}

fn button(y: f32, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: pos2(100.0, y),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

fn wheel() -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 24.0),
        modifiers: egui::Modifiers::default(),
    }
}

fn start<P: Param>(ctx: &egui::Context, param: &P, host: &RecordingHost) {
    frame(
        ctx,
        param,
        host,
        vec![egui::Event::PointerMoved(pos2(100.0, 100.0))],
        false,
    );
    frame(ctx, param, host, vec![button(100.0, true)], false);
}

fn values(host: &RecordingHost) -> Vec<f32> {
    host.0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            Event::Set(value) => Some(*value),
            _ => None,
        })
        .collect()
}

fn assert_one_gesture(host: &RecordingHost) {
    let events = host.0.lock().unwrap();
    assert_eq!(events.first(), Some(&Event::Begin));
    assert_eq!(events.last(), Some(&Event::End));
    assert_eq!(events.iter().filter(|e| **e == Event::Begin).count(), 1);
    assert_eq!(events.iter().filter(|e| **e == Event::End).count(), 1);
}

#[test]
fn drag_has_one_gesture_and_accumulates_despite_delayed_host_feedback() {
    let ctx = egui::Context::default();
    let host = RecordingHost::default();
    let param = FloatParam::new("Test", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 });
    start(&ctx, &param, &host);
    for y in [90.0, 80.0, 70.0, 80.0, 90.0] {
        frame(
            &ctx,
            &param,
            &host,
            vec![egui::Event::PointerMoved(pos2(100.0, y))],
            false,
        );
    }
    frame(&ctx, &param, &host, vec![button(90.0, false)], false);
    assert_one_gesture(&host);
    let actual = values(&host);
    assert_eq!(actual.len(), 5);
    for (actual, expected) in actual.iter().zip([0.54, 0.58, 0.62, 0.58, 0.54]) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }
}

#[test]
fn shift_drag_uses_fine_sensitivity_and_ignores_wheel_during_gesture() {
    let ctx = egui::Context::default();
    let host = RecordingHost::default();
    let param = FloatParam::new("Test", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 });
    start(&ctx, &param, &host);
    frame(
        &ctx,
        &param,
        &host,
        vec![egui::Event::PointerMoved(pos2(100.0, 90.0)), wheel()],
        true,
    );
    frame(
        &ctx,
        &param,
        &host,
        vec![egui::Event::PointerMoved(pos2(100.0, 80.0))],
        true,
    );
    frame(&ctx, &param, &host, vec![button(80.0, false)], true);
    assert_one_gesture(&host);
    let actual = values(&host);
    assert_eq!(actual.len(), 2);
    assert!((actual[0] - 0.51).abs() < 1e-6);
    assert!((actual[1] - 0.52).abs() < 1e-6);
}

#[test]
fn discrete_drag_retains_fractional_motion_and_clamps_at_both_ends() {
    let ctx = egui::Context::default();
    let host = RecordingHost::default();
    let param = IntParam::new("BOUNCES", 1, IntRange::Linear { min: 1, max: 4 });
    start(&ctx, &param, &host);
    for y in (0..=90).rev() {
        frame(
            &ctx,
            &param,
            &host,
            vec![egui::Event::PointerMoved(pos2(100.0, y as f32))],
            false,
        );
    }
    let increasing = values(&host);
    assert!(increasing.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(param.preview_plain(*increasing.last().unwrap()), 2);
    frame(
        &ctx,
        &param,
        &host,
        vec![egui::Event::PointerMoved(pos2(100.0, -1000.0))],
        false,
    );
    frame(
        &ctx,
        &param,
        &host,
        vec![egui::Event::PointerMoved(pos2(100.0, 1000.0))],
        false,
    );
    frame(&ctx, &param, &host, vec![button(1000.0, false)], false);
    assert_one_gesture(&host);
    let actual = values(&host);
    assert_eq!(&actual[actual.len() - 2..], &[1.0, 0.0]);
}

#[test]
fn bounces_wheel_visits_every_integer_and_clamps() {
    let param = IntParam::new("BOUNCES", 1, IntRange::Linear { min: 1, max: 4 });
    let mut normalized = 0.0;
    for expected in [2, 3, 4, 4] {
        normalized = knob_scroll_value(&param, normalized, 1.0);
        assert_eq!(param.preview_plain(normalized), expected);
        assert_eq!(normalized, param.preview_normalized(expected));
    }
    for expected in [3, 2, 1, 1] {
        normalized = knob_scroll_value(&param, normalized, -1.0);
        assert_eq!(param.preview_plain(normalized), expected);
        assert_eq!(normalized, param.preview_normalized(expected));
    }
}

#[test]
fn wheel_and_double_click_are_balanced_independent_gestures() {
    let ctx = egui::Context::default();
    let host = RecordingHost::default();
    let param = FloatParam::new("Test", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 });
    frame(
        &ctx,
        &param,
        &host,
        vec![egui::Event::PointerMoved(pos2(100.0, 100.0))],
        false,
    );
    frame(&ctx, &param, &host, vec![wheel()], false);
    assert_one_gesture(&host);
    assert!((values(&host)[0] - 0.52).abs() < 1e-6);
    host.0.lock().unwrap().clear();
    for _ in 0..2 {
        frame(&ctx, &param, &host, vec![button(100.0, true)], false);
        frame(&ctx, &param, &host, vec![button(100.0, false)], false);
    }
    assert_one_gesture(&host);
    assert_eq!(values(&host), vec![param.default_normalized_value()]);
}
