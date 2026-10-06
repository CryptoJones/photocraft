//! Ink-only diagnostics: the same window-event batches through the old and new input paths.
//! Frame receipt times are recorded, not invented device timestamps or raw trackpad contacts.

use egui::{Event, Modifiers};
use photocraft_engine::paint::BrushSettings;
use serde::{Deserialize, Serialize};

use crate::canvas::ViewXform;
use crate::stylus::PenSample;
use crate::{PhotocraftApp, Tool};

#[derive(Clone, Serialize, Deserialize)]
pub struct Recording {
    version: u8,
    size: [u32; 2],
    depth: u8,
    tool: Tool,
    brush: BrushSettings,
    foreground: [f32; 4],
    background: [f32; 4],
    right_erase: bool,
    auto_erase: bool,
    frames: Vec<Frame>,
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
            r.tool != tool || r.brush != app.session.tools.brush || r.foreground != app.session.tools.foreground || r.background != app.session.tools.background
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

}
#[cfg(test)]
mod tests {
    use super::*;
    use egui::{PointerButton, Rect, pos2, vec2};

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
    fn recording_round_trip_and_invalid_frames() {
        let mut r = recording(8, 0.0);
        let bytes = serde_json::to_vec(&r).unwrap();
        assert_eq!(Recording::load(&bytes).unwrap().frames.len(), 4);
        r.version = 99;
        assert!(r.validate().is_err());
        r.version = 1;
        r.frames[0].xf.zoom = 0.0;
        assert!(r.validate().is_err());
    }
}
