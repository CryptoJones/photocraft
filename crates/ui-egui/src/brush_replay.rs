//! Ink-only diagnostics: the same window-event batches through the old and new input paths.
//! Frame receipt times are recorded, not invented device timestamps or raw trackpad contacts.

use egui::{Context, Event, Modifiers, RawInput, Sense};
use photocraft_engine::{Session, paint::BrushSettings};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::canvas::{ToolEvent, ViewXform, tool_event};
use crate::stylus::PenSample;
use crate::{PhotocraftApp, Services, Tool};

#[derive(Clone, Serialize, Deserialize)]
pub struct Recording {
    version: u8,
    size: [u32; 2],
    depth: u8,
    tool: Tool,
    brush: BrushSettings,
    foreground: [f32; 4],
    background: [f32; 4],
    #[serde(default = "pressure_enabled")]
    use_pressure: bool,
    right_erase: bool,
    auto_erase: bool,
    frames: Vec<Frame>,
}

fn pressure_enabled() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
struct Frame {
    time: f64,
    events: Vec<Event>,
    modifiers: Modifiers,
    xf: ViewXform,
    eligible: bool,
    pen: Option<PenSample>,
}

#[derive(Default)]
pub struct Lab {
    pub open: bool,
    pub capturing: bool,
    pub replay_without_smoothing: bool,
    comparison: Option<String>,
    pub recording: Option<Recording>,
    pub cpu_docs: Vec<photocraft_doc::DocId>,
    source: Option<photocraft_doc::DocId>,
    events: usize,
}

impl Lab {
    pub fn start(app: &mut PhotocraftApp) -> Result<(), String> {
        if app.drag.is_some() {
            return Err("Finish the current stroke first".into());
        }
        if !matches!(app.ui.tool, Tool::Brush | Tool::Pencil | Tool::Eraser) {
            return Err("Select Brush, Pencil or Eraser first".into());
        }
        let doc = &app.session.active().ok_or("Open a canvas first")?.doc;
        if doc.size.width > 4096 || doc.size.height > 4096 {
            return Err("Use a canvas of at most 4096 × 4096 for brush recording".into());
        }
        app.brush_lab.recording = Some(Recording {
            version: 1,
            size: [doc.size.width, doc.size.height],
            depth: match doc.depth {
                photocraft_color::SampleType::U8 => 8,
                photocraft_color::SampleType::U16 => 16,
                photocraft_color::SampleType::F32 => 32,
            },
            tool: app.ui.tool,
            brush: app.session.tools.brush.clone(),
            foreground: app.session.tools.foreground,
            background: app.session.tools.background,
            use_pressure: app.stylus.use_pressure,
            right_erase: crate::paint_mouse::right_erases(app, app.ui.tool),
            auto_erase: app.ui.tool_options.pencil_auto_erase,
            frames: Vec::new(),
        });
        app.brush_lab.source = Some(doc.id);
        app.brush_lab.events = 0;
        app.brush_lab.capturing = true;
        Ok(())
    }

    pub fn capture(app: &mut PhotocraftApp, response: &egui::Response, xf: &ViewXform, tool: Tool) {
        if !app.brush_lab.capturing {
            return;
        }
        if app.brush_lab.recording.as_ref().is_none_or(|r| {
            r.tool != tool
                || r.brush != app.session.tools.brush
                || r.foreground != app.session.tools.foreground
                || r.background != app.session.tools.background
                || r.use_pressure != app.stylus.use_pressure
                || r.right_erase != crate::paint_mouse::right_erases(app, tool)
                || r.auto_erase != app.ui.tool_options.pencil_auto_erase
        }) || app.session.active().map(|s| s.doc.id) != app.brush_lab.source
        {
            app.brush_lab.capturing = false;
            app.ui.status = "Brush recording stopped: brush, colour, tool or canvas changed".into();
            return;
        }
        let (time, modifiers, events) = response.ctx.input(|i| {
            (
                i.time,
                i.modifiers,
                i.raw
                    .events
                    .iter()
                    .filter(|e| {
                        matches!(
                            e,
                            Event::PointerMoved(_)
                                | Event::PointerButton { .. }
                                | Event::Touch { .. }
                                | Event::Key { .. }
                                | Event::ModifiersChanged(_)
                                | Event::WindowFocused(_)
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        });
        if events.is_empty() {
            return;
        }
        let eligible = response.hovered() || response.dragged() || response.drag_stopped() || response.is_pointer_button_down_on();
        let frame = Frame { time, events, modifiers, xf: *xf, eligible, pen: app.stylus.sample() };
        if let Some(recording) = &mut app.brush_lab.recording {
            app.brush_lab.events += frame.events.len();
            if recording.frames.len() >= 10000 || app.brush_lab.events > 100000 {
                app.brush_lab.capturing = false;
                app.ui.status = "Brush recording reached its sample limit".into();
            } else {
                recording.frames.push(frame);
            }
        }
    }
}

impl Recording {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Brush recording exceeds 16 MiB".into());
        }
        let record: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.size.iter().any(|s| *s == 0 || *s > 4096)
            || ![8, 16, 32].contains(&self.depth)
            || self.frames.len() > 10000
            || self.frames.iter().map(|f| f.events.len()).sum::<usize>() > 100000
        {
            return Err("Unsupported or oversized brush recording".into());
        }
        let mut previous = 0.0;
        for f in &self.frames {
            if !f.time.is_finite()
                || f.time < previous
                || !f.xf.zoom.is_finite()
                || !(0.01..=64.0).contains(&f.xf.zoom)
                || !f.xf.rect.is_finite()
                || !f.xf.rect.is_positive()
                || f.xf.rect.size().max_elem() > 10000.0
                || f.xf.center.iter().any(|p| !p.is_finite())
                || !matches!(self.tool, Tool::Brush | Tool::Pencil | Tool::Eraser)
            {
                return Err("Invalid brush recording frame".into());
            }
            if f.events.iter().any(|e| match e {
                Event::PointerMoved(p) | Event::PointerButton { pos: p, .. } => !p.is_finite(),
                Event::Touch { pos, force, .. } => !pos.is_finite() || force.is_some_and(|v| !v.is_finite()),
                Event::WindowFocused(_) | Event::Key { .. } | Event::ModifiersChanged(_) => false,
                _ => true,
            }) {
                return Err("Invalid brush recording event".into());
            }
            previous = f.time;
        }
        Ok(())
    }

    fn replay(&self, all_samples: bool) -> Result<PhotocraftApp, String> {
        self.validate()?;
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": self.size[0], "height": self.size[1], "depth": self.depth, "background": "transparent"}))?;
        // A Photoshop Background layer erases to the background colour. Use an ordinary
        // opaque layer here so eraser replays show their path as transparent holes.
        if self.tool == Tool::Eraser {
            app.run("edit.fill", json!({"contents": "white"}))?;
        }
        app.sync_views();
        app.brush_input.all_samples = all_samples;
        let ctx = Context::default();
        app.stylus.use_pressure = self.use_pressure;
        app.ui.tool = self.tool;
        app.ui.smoothing_tool = Some(self.tool);
        app.session.tools.brush = self.brush.clone();
        app.session.tools.foreground = self.foreground;
        app.session.tools.background = self.background;
        app.ui.tool_options.pencil_auto_erase = self.auto_erase;
        app.session.edit_prefs(|p| {
            p.tools.right_click_with_painting_tools =
                if self.right_erase { photocraft_engine::prefs::RightClickPaint::Erase } else { photocraft_engine::prefs::RightClickPaint::BrushPicker }
        });
        for f in &self.frames {
            if let Some(view) = app.ui.views.first_mut() {
                view.zoom = f.xf.zoom;
            }
            app.stylus.feed.set(f.pen);
            app.stylus.update(&f.events);
            let mut events = vec![Event::ModifiersChanged(f.modifiers)];
            events.extend(f.events.clone());
            let input = RawInput { time: Some(f.time), events, screen_rect: Some(f.xf.rect.expand(100.0)), ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let sense = if f.eligible || app.drag.is_some() { Sense::click_and_drag() } else { Sense::hover() };
                let response = ui.allocate_rect(f.xf.rect, sense);
                let buttons = crate::paint_mouse::canvas_buttons(&mut app, &response, self.tool);
                if all_samples {
                    crate::brush_input::route_with_eligibility(&mut app, &response, &f.xf, self.tool, f.eligible);
                } else {
                    legacy(&mut app, &response, &f.xf, buttons);
                }
            });
            output.textures_delta.clear();
            if app.ui.status_error {
                return Err(app.ui.status);
            }
        }
        if app.drag.is_some() {
            return Err("Recording ends during a stroke; record its release too".into());
        }
        if app.ui.status_error {
            return Err(app.ui.status);
        }
        Ok(app)
    }
}

/// The original canvas Brush/Pencil/Eraser response path, kept for replay comparisons.
fn legacy(app: &mut PhotocraftApp, response: &egui::Response, xf: &ViewXform, buttons: crate::paint_mouse::Buttons) {
    let mods = response.ctx.input(|i| i.modifiers);
    let pressure = app.stylus.pressure();
    if buttons.started
        && let Some(p) = response.ctx.input(|i| i.pointer.press_origin()).filter(|p| xf.rect.contains(*p)).or(response.interact_pointer_pos())
    {
        let [x, y] = xf.to_doc(p);
        tool_event(app, ToolEvent::Down { x, y, pressure }, mods);
    }
    if buttons.dragged
        && let Some(p) = response.interact_pointer_pos()
    {
        let [x, y] = xf.to_doc(p);
        tool_event(app, ToolEvent::Move { x, y, pressure }, mods);
    }
    if buttons.stopped {
        let point = response.interact_pointer_pos().map(|p| xf.to_doc(p)).or_else(|| app.drag.as_ref().and_then(|d| d.points.last().map(|q| [q[0], q[1]])));
        if let Some([x, y]) = point {
            tool_event(app, ToolEvent::Up { x, y }, mods);
        }
    }
    if buttons.clicked
        && let Some(p) = response.interact_pointer_pos()
    {
        let [x, y] = xf.to_doc(p);
        tool_event(app, ToolEvent::Down { x, y, pressure: 1.0 }, mods);
        tool_event(app, ToolEvent::Up { x, y }, mods);
    }
}

pub fn compare(app: &mut PhotocraftApp) -> Result<(), String> {
    if app.drag.is_some() || app.brush_lab.capturing {
        return Err("Finish the stroke and stop recording first".into());
    }
    let record = app.brush_lab.recording.as_ref().ok_or("Record a scribble first")?;
    if record.frames.is_empty() {
        return Err("The recording has no input".into());
    }
    let mut record = record.clone();
    if app.brush_lab.replay_without_smoothing {
        record.brush.smoothing.amount = 0.0;
        record.brush.smoothing.pulled_string = false;
    }
    let old = record.replay(false)?;
    let new = record.replay(true)?;
    let points = |source: &PhotocraftApp| source.session.journal.iter()
        .filter(|(id, _)| id == "paint.stroke")
        .filter_map(|(_, args)| args["points"].as_array()).map(Vec::len).sum::<usize>();
    app.brush_lab.comparison = Some(format!("Stroke points: old {}, new {} · replay smoothing {}%",
        points(&old), points(&new), record.brush.smoothing.amount * 100.0));
    let first = app.session.documents().len();
    for cpu in [true, false] {
        for (label, source) in [("Old frame samples", &old), ("All motion samples", &new)] {
            let mut doc = (*source.session.active().ok_or("Replay has no document")?.doc).clone();
            doc.name = format!(
                "{label} · {}",
                if cpu {
                    "CPU"
                } else if app.gpu.is_some() {
                    "GPU"
                } else {
                    "CPU (GPU unavailable)"
                }
            );
            let i = app.session.add_document(doc, None);
            if cpu && let Some(state) = app.session.documents().get(i) {
                app.brush_lab.cpu_docs.push(state.doc.id);
            }
        }
    }
    if !app.session.set_active(first) {
        return Err("Replay document disappeared".into());
    }
    app.sync_views();
    app.ui.view.arrange = "fourUp".into();
    Ok(())
}

pub fn show(app: &mut PhotocraftApp, ctx: &Context) {
    let mut open = app.brush_lab.open;
    let mut action = None;
    egui::Window::new("Brush Input Lab").open(&mut open).show(ctx, |ui| {
        ui.add_enabled_ui(app.drag.is_none() && !app.brush_lab.capturing, |ui| {
            ui.checkbox(&mut app.brush_input.all_samples, "Use all motion samples");
            ui.checkbox(&mut app.brush_cursor.native, "Native brush cursor");
        });
        ui.label("Replay uses blank RGB canvases and the original frame batches.");
        ui.checkbox(&mut app.brush_lab.replay_without_smoothing, "Replay with smoothing off");
        if let Some(summary) = &app.brush_lab.comparison {
            ui.label(summary);
        }
        ui.horizontal(|ui| {
            if ui.button(if app.brush_lab.capturing { "Stop recording" } else { "Record new scribble" }).clicked() {
                action = Some("record");
            }
            if ui.button("Compare old / new · CPU / GPU").clicked() {
                action = Some("compare");
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Save recording…").clicked() {
                action = Some("save");
            }
            if ui.button("Load recording…").clicked() {
                action = Some("load");
            }
        });
        if let Some(r) = &app.brush_lab.recording {
            let moves = r.frames.iter().map(|f| f.events.iter().filter(|e| matches!(e, Event::PointerMoved(_))).count()).collect::<Vec<_>>();
            ui.label(format!("{} input batches · {} motion events · {} batches with multiple moves",
                r.frames.len(), moves.iter().sum::<usize>(), moves.iter().filter(|n| **n > 1).count()));
            ui.label("Window events only; this does not capture raw hardware samples.");
        }
    });
    app.brush_lab.open = open;
    let result = match action {
        Some("record") if app.brush_lab.capturing => {
            app.brush_lab.capturing = false;
            Ok(())
        }
        Some("record") => Lab::start(app),
        Some("compare") => compare(app),
        Some("save") => save(app),
        Some("load") => load(app),
        _ => Ok(()),
    };
    if let Err(e) = result {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}

fn save(app: &mut PhotocraftApp) -> Result<(), String> {
    if app.brush_lab.capturing || app.drag.is_some() {
        return Err("Finish the stroke and stop recording first".into());
    }
    let record = app.brush_lab.recording.as_ref().ok_or("Record a scribble first")?;
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Brush recording exceeds 16 MiB; record a shorter scribble".into());
    }
    if let Some(path) = app.services.pick_save.as_mut().and_then(|p| p("scribble.brush-replay.json")) {
        app.services.write.as_mut().ok_or("No file writer")?(&path, &bytes)?;
    }
    Ok(())
}

fn load(app: &mut PhotocraftApp) -> Result<(), String> {
    if app.brush_lab.capturing || app.drag.is_some() {
        return Err("Stop recording and finish the stroke first".into());
    }
    if let Some((_, bytes)) = app.services.pick_brush_recording.as_mut().ok_or("Recording file picker unavailable; use ui.brushReplay.load")?() {
        app.brush_lab.recording = Some(Recording::load(&bytes)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{PointerButton, Pos2, Rect, pos2, vec2};

    fn recording(depth: u8, smoothing: f32) -> Recording {
        let xf = ViewXform { rect: Rect::from_min_size(pos2(20.0, 20.0), vec2(256.0, 256.0)), zoom: 1.0, center: [128.0; 2], flip: false };
        let a = pos2(60.0, 140.0);
        let b = pos2(240.0, 140.0);
        let button = |pos, pressed| Event::PointerButton { pos, pressed, button: PointerButton::Primary, modifiers: Modifiers::NONE };
        let mut brush = BrushSettings { size: 6.0, hardness: 1.0, ..Default::default() };
        brush.smoothing.amount = smoothing;
        let events = [
            vec![Event::PointerMoved(a)],
            vec![button(a, true)],
            vec![Event::PointerMoved(pos2(100.0, 220.0)), Event::PointerMoved(pos2(180.0, 60.0)), Event::PointerMoved(b)],
            vec![button(b, false)],
        ];
        Recording {
            version: 1,
            size: [256, 256],
            depth,
            tool: Tool::Brush,
            brush,
            foreground: [0.0, 0.0, 0.0, 1.0],
            background: [1.0; 4],
            use_pressure: true,
            right_erase: false,
            auto_erase: false,
            frames: events
                .into_iter()
                .enumerate()
                .map(|(i, events)| Frame { time: 1.0 + i as f64 / 60.0, events, modifiers: Modifiers::NONE, xf, eligible: true, pen: None })
                .collect(),
        }
    }

    #[test]
    fn replay_is_deterministic_and_retains_curve_across_depths_and_smoothing() {
        for depth in [8, 16, 32] {
            for smoothing in [0.0, 0.1, 0.6] {
                let r = recording(depth, smoothing);
                let bytes = serde_json::to_vec(&r).unwrap();
                let loaded = Recording::load(&bytes).unwrap();
                let old = loaded.replay(false).unwrap();
                let new = loaded.replay(true).unwrap();
                let again = loaded.replay(true).unwrap();
                let pixels = |app: &PhotocraftApp| photocraft_compose::flatten(&app.session.active().unwrap().doc).to_rgba8().pixels;
                assert_ne!(pixels(&old), pixels(&new), "depth {depth}, smoothing {smoothing}");
                assert_eq!(pixels(&new), pixels(&again));
                let points =
                    |app: &PhotocraftApp| app.session.journal.iter().find(|(id, _)| id == "paint.stroke").unwrap().1["points"].as_array().unwrap().len();
                assert_eq!(points(&old), 2);
                assert_eq!(points(&new), 4);
            }
        }
    }

    #[test]
    fn rejects_invalid_and_incomplete_recordings() {
        let mut r = recording(8, 0.0);
        r.version = 99;
        assert!(r.replay(true).is_err());
        r.version = 1;
        r.frames[0].xf.zoom = 0.0;
        assert!(r.replay(true).is_err());
        r.frames[0].xf.zoom = 1.0;
        r.frames[0].events.push(Event::PointerMoved(Pos2::new(f32::INFINITY, 0.0)));
        assert!(r.replay(true).is_err());
        let mut r = recording(8, 0.0);
        r.frames.pop();
        assert!(r.replay(true).is_err());
    }

    #[test]
    fn pencil_and_eraser_replays_show_the_sampling_difference() {
        for tool in [Tool::Pencil, Tool::Eraser] {
            let mut r = recording(8, 0.0);
            r.tool = tool;
            let old = r.replay(false).unwrap();
            let new = r.replay(true).unwrap();
            let pixels = |app: &PhotocraftApp| photocraft_compose::flatten(&app.session.active().unwrap().doc).to_rgba8().pixels;
            assert_ne!(pixels(&old), pixels(&new), "{tool:?}");
            if tool == Tool::Eraser {
                assert!(pixels(&new).chunks_exact(4).any(|p| p[3] == 0), "erasure is visible");
                assert!(pixels(&new).chunks_exact(4).any(|p| p[3] == 255), "the rest remains opaque");
            }
        }
    }

    #[test]
    fn comparison_keeps_original_document_and_opens_four_render_views() {
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 128, "height": 128})).unwrap();
        let original = app.session.active().unwrap().doc.clone();
        app.brush_lab.recording = Some(recording(8, 0.0));
        compare(&mut app).unwrap();
        assert_eq!(app.session.documents().len(), 5);
        assert!(std::sync::Arc::ptr_eq(&original, &app.session.documents()[0].doc));
        assert_eq!(app.brush_lab.cpu_docs.len(), 2);
        assert_eq!(app.ui.view.arrange, "fourUp");
        // Deterministic synthetic evidence; this is not a human trackpad recording.
        if let Ok(path) = std::env::var("PHOTOCRAFT_REPLAY_FIXTURE") {
            std::fs::write(path, serde_json::to_vec(app.brush_lab.recording.as_ref().unwrap()).unwrap()).unwrap();
            for (i, label) in [(1, "old"), (2, "new")] {
                let pixels = photocraft_compose::flatten(&app.session.documents()[i].doc).to_rgba8().pixels;
                if let Ok(dir) = std::env::var("PHOTOCRAFT_REPLAY_EVIDENCE") {
                    std::fs::write(format!("{dir}/{label}.rgba"), pixels).unwrap();
                }
            }
        }
    }

    #[test]
    fn native_file_picker_round_trips_recording_and_bad_load_preserves_it() {
        let bytes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = bytes.clone();
        let mut app = PhotocraftApp::new(
            Session::new(),
            Services {
                pick_save: Some(Box::new(|name| Some(name.into()))),
                write: Some(Box::new(move |_, value| {
                    *output.lock().unwrap() = value.to_vec();
                    Ok(())
                })),
                ..Default::default()
            },
        );
        app.brush_lab.recording = Some(recording(8, 0.0));
        save(&mut app).unwrap();
        let input = bytes.clone();
        app.services.pick_brush_recording = Some(Box::new(move || Some(("scribble.brush-replay.json".into(), input.lock().unwrap().clone()))));
        app.brush_lab.recording = None;
        load(&mut app).unwrap();
        assert_eq!(app.brush_lab.recording.as_ref().unwrap().frames.len(), 4);
        *bytes.lock().unwrap() = b"not JSON".to_vec();
        assert!(load(&mut app).is_err());
        assert_eq!(app.brush_lab.recording.as_ref().unwrap().frames.len(), 4);
    }

    #[test]
    fn actual_canvas_records_batches_and_replays_saved_scribble() {
        use egui_kittest::Harness;
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 256, "height": 256})).unwrap();
        app.ui.tool = Tool::Brush;
        let mut h = Harness::builder().with_size(vec2(1000.0, 800.0)).build_eframe(|_| app);
        h.run_steps(3);
        Lab::start(h.state_mut()).unwrap();
        let c = h.state().last_canvas_rect.center();
        let button = |pos, pressed| Event::PointerButton { pos, pressed, button: PointerButton::Primary, modifiers: Modifiers::NONE };
        for events in [
            vec![Event::PointerMoved(c)],
            vec![button(c, true)],
            vec![Event::PointerMoved(c + vec2(30.0, 40.0)), Event::PointerMoved(c + vec2(60.0, -40.0))],
            vec![button(c + vec2(60.0, -40.0), false)],
        ] {
            h.input_mut().events.extend(events);
            h.step();
        }
        h.state_mut().brush_lab.capturing = false;
        let record = h.state().brush_lab.recording.as_ref().unwrap();
        assert_eq!(record.frames.len(), 4);
        let replay = record.replay(true).unwrap();
        let points = |app: &PhotocraftApp| app.session.journal.iter().find(|(id, _)| id == "paint.stroke").unwrap().1["points"].clone();
        assert_eq!(points(h.state()), points(&replay));
    }
}
