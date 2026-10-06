//! Painting consumes ordered window events, rather than just the last position of a frame.
//! Absolute OS positions retain the trackpad's acceleration and match the native cursor.

use egui::{Event, Modifiers, PointerButton, Response};

use crate::PhotocraftApp;
use crate::canvas::{ToolEvent, ViewXform, tool_event};
use crate::state::Tool;

pub struct BrushInput {
    pub all_samples: bool,
    pub defer_preview: bool,
    button: Option<PointerButton>,
    last: Option<egui::Pos2>,
}

impl Default for BrushInput {
    fn default() -> Self {
        Self { all_samples: true, defer_preview: false, button: None, last: None }
    }
}

/// Returns true when this path owns painting; other tools retain their response-based gestures.
pub fn route(app: &mut PhotocraftApp, response: &Response, xf: &ViewXform, tool: Tool) -> bool {
    crate::brush_replay::Lab::capture(app, response, xf, tool);
    let eligible = response.hovered() || response.dragged() || response.drag_stopped() || response.is_pointer_button_down_on();
    route_with_eligibility(app, response, xf, tool, eligible)
}

pub fn route_with_eligibility(app: &mut PhotocraftApp, response: &Response, xf: &ViewXform, tool: Tool, eligible: bool) -> bool {
    if !app.brush_input.all_samples || !tool.is_brushlike() && tool != Tool::QuickSelection {
        return false;
    }
    let (events, mods) = response
        .ctx
        .input(|i| (i.raw.events.iter().filter(|e| app.stylus.use_pressure || !matches!(e, Event::Touch { .. })).cloned().collect::<Vec<_>>(), i.modifiers));
    let secondary = crate::paint_mouse::right_erases(app, tool);
    let pressure = app.stylus.pressure();
    let samples = app.brush_input.events(&events, xf, eligible, secondary, pressure, mods);
    app.brush_input.defer_preview = true;
    for (event, modifiers, erase) in samples {
        if matches!(event, ToolEvent::Up { .. }) {
            crate::canvas::feed_live_stroke(app);
        }
        if matches!(event, ToolEvent::Down { .. }) {
            app.secondary_erase = erase;
        }
        tool_event(app, event, modifiers);
    }
    app.brush_input.defer_preview = false;
    crate::canvas::feed_live_stroke(app);
    true
}

impl BrushInput {
    fn events(
        &mut self,
        events: &[Event],
        xf: &ViewXform,
        eligible: bool,
        secondary: bool,
        mut pressure: f32,
        mut mods: Modifiers,
    ) -> Vec<(ToolEvent, Modifiers, bool)> {
        let mut out = Vec::new();
        for event in events {
            match *event {
                Event::PointerButton { pos, button, pressed: true, modifiers }
                    if self.button.is_none()
                        && eligible
                        && xf.rect.contains(pos)
                        && (button == PointerButton::Primary || secondary && button == PointerButton::Secondary) =>
                {
                    self.button = Some(button);
                    self.last = Some(pos);
                    mods = modifiers;
                    let [x, y] = xf.to_doc(pos);
                    out.push((ToolEvent::Down { x, y, pressure }, mods, button == PointerButton::Secondary));
                }
                Event::PointerMoved(pos) if self.button.is_some() => {
                    self.last = Some(pos);
                    let [x, y] = xf.to_doc(pos);
                    out.push((ToolEvent::Move { x, y, pressure }, mods, false));
                }
                Event::PointerButton { pos, button, pressed: false, modifiers } if self.button == Some(button) => {
                    self.button = None;
                    let [x, y] = xf.to_doc(pos);
                    out.push((ToolEvent::Up { x, y }, modifiers, false));
                }
                Event::WindowFocused(false) if self.button.take().is_some() => {
                    if let Some(pos) = self.last {
                        let [x, y] = xf.to_doc(pos);
                        out.push((ToolEvent::Up { x, y }, mods, false));
                    }
                }
                Event::Touch { force: Some(force), .. } if force.is_finite() => pressure = force.clamp(0.0, 1.0),
                Event::Key { modifiers, .. } | Event::ModifiersChanged(modifiers) => mods = modifiers,
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, pos2};

    use super::*;

    fn xf() -> ViewXform {
        ViewXform { rect: Rect::from_min_max(Pos2::ZERO, pos2(100.0, 100.0)), zoom: 2.0, center: [50.0; 2], flip: true }
    }

    fn button(pos: egui::Pos2, pressed: bool, button: PointerButton) -> Event {
        Event::PointerButton { pos, pressed, button, modifiers: Modifiers::NONE }
    }

    #[test]
    fn preserves_fast_curve_and_press_release_in_one_frame() {
        let p = pos2(20.0, 20.0);
        let events = [
            button(p, true, PointerButton::Primary),
            Event::PointerMoved(pos2(40.0, 80.0)),
            Event::PointerMoved(pos2(80.0, 20.0)),
            button(p, false, PointerButton::Primary),
        ];
        let mut input = BrushInput::default();
        let out = input.events(&events, &xf(), true, false, 0.4, Modifiers::NONE);
        assert_eq!(out.len(), 4);
        assert_eq!(out[1].0, ToolEvent::Move { x: 55.0, y: 65.0, pressure: 0.4 });
        assert!(input.button.is_none());
    }

    #[test]
    fn only_captures_canvas_presses_and_matching_releases() {
        let p = pos2(20.0, 20.0);
        let events = [button(p, true, PointerButton::Primary), Event::PointerMoved(p)];
        let mut input = BrushInput::default();
        assert!(input.events(&events, &xf(), false, false, 1.0, Modifiers::NONE).is_empty());
        let right = [button(p, true, PointerButton::Secondary)];
        assert!(input.events(&right, &xf(), true, false, 1.0, Modifiers::NONE).is_empty());
        assert_eq!(input.events(&right, &xf(), true, true, 1.0, Modifiers::NONE).len(), 1);
        // Captured strokes continue beyond the canvas; the other button cannot finish them.
        let outside = [Event::PointerMoved(pos2(200.0, 200.0)), button(p, false, PointerButton::Primary)];
        assert_eq!(input.events(&outside, &xf(), false, true, 1.0, Modifiers::NONE).len(), 1);
        assert!(input.button.is_some());
        input.events(&[button(p, false, PointerButton::Secondary)], &xf(), false, true, 1.0, Modifiers::NONE);
        assert!(input.button.is_none());
    }

    #[test]
    fn focus_loss_finishes_capture() {
        let p = pos2(20.0, 20.0);
        let mut input = BrushInput::default();
        let events = [button(p, true, PointerButton::Primary), Event::PointerMoved(p), Event::WindowFocused(false)];
        let out = input.events(&events, &xf(), true, false, 1.0, Modifiers::NONE);
        assert!(matches!(out.last().unwrap().0, ToolEvent::Up { .. }));
        assert!(input.button.is_none());
    }

    #[test]
    fn touch_force_stays_in_event_order() {
        let p = pos2(20.0, 20.0);
        let touch = |force| Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(0), phase: egui::TouchPhase::Move, pos: p, force: Some(force) };
        let events = [button(p, true, PointerButton::Primary), touch(0.2), Event::PointerMoved(p), touch(0.8), Event::PointerMoved(p)];
        let out = BrushInput::default().events(&events, &xf(), true, false, 1.0, Modifiers::NONE);
        assert!(matches!(out[1].0, ToolEvent::Move { pressure: 0.2, .. }));
        assert!(matches!(out[2].0, ToolEvent::Move { pressure: 0.8, .. }));
    }

    #[test]
    fn disabled_tablet_pressure_stays_disabled_with_batched_input() {
        use egui_kittest::Harness;
        use serde_json::json;
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 128, "height": 128})).unwrap();
        app.run("prefs.set", json!({"path":"tools.useTabletPressure", "value":false})).unwrap();
        app.ui.tool = Tool::Brush;
        let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_eframe(|_| app);
        h.run_steps(3);
        let p = h.state().last_canvas_rect.center();
        h.input_mut().events.extend([
            Event::PointerMoved(p),
            button(p, true, PointerButton::Primary),
            Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(0), phase: egui::TouchPhase::Move, pos: p, force: Some(0.2) },
            Event::PointerMoved(p + egui::vec2(20.0, 0.0)),
            button(p + egui::vec2(20.0, 0.0), false, PointerButton::Primary),
        ]);
        h.step();
        let points = &h.state().session.journal.iter().find(|(id, _)| id == "paint.stroke").unwrap().1["points"];
        assert!(points.as_array().unwrap().iter().all(|p| p[2] == 1.0));
    }

    #[test]
    fn real_canvas_preserves_batched_curve_compared_with_old_path() {
        use egui_kittest::Harness;
        use serde_json::json;
        for all in [false, true] {
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({"width": 256, "height": 256, "background": "transparent"})).unwrap();
            app.ui.tool = Tool::Brush;
            app.session.tools.brush.size = 6.0;
            app.session.tools.brush.smoothing.amount = 0.0;
            app.brush_input.all_samples = all;
            let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_eframe(|_| app);
            h.run_steps(3);
            let c = h.state().last_canvas_rect.center();
            let a = c - egui::vec2(60.0, 0.0);
            h.event(Event::PointerMoved(a));
            h.run_steps(1);
            h.event(button(a, true, PointerButton::Primary));
            h.run_steps(1);
            for p in [c + egui::vec2(-30.0, 50.0), c + egui::vec2(30.0, -50.0), c + egui::vec2(60.0, 0.0)] {
                h.input_mut().events.push(Event::PointerMoved(p));
            }
            h.run_steps(1);
            h.event(button(c + egui::vec2(60.0, 0.0), false, PointerButton::Primary));
            h.run_steps(1);
            let strokes: Vec<_> = h.state().session.journal.iter().filter(|(id, _)| id == "paint.stroke").collect();
            assert_eq!(strokes.len(), 1, "one undo step");
            assert_eq!(strokes[0].1["points"].as_array().unwrap().len(), if all { 4 } else { 2 });
        }
    }
}
