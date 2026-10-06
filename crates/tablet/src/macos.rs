//! AppKit local event monitor for tablet data. The only module in the workspace that allows
//! `unsafe`: three calls into AppKit's Objective-C API that objc2 can't prove sound on its own
//! (each has a `SAFETY:` comment). Everything the monitor reads goes through the pure, tested
//! mapping in [`crate::appkit`].
//!
//! A *local* monitor sees this app's events only, on the main thread, inside `-[NSApplication
//! sendEvent:]` before the event reaches winit's view, so the sample is current when the UI
//! handles the matching pointer event. The monitor passes every event through unchanged.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask};

use crate::appkit::{RawEvent, State};
use crate::{Error, Sample, Update, deliver};

/// Keeps the monitor installed; dropping it removes the monitor. Main-thread only (not `Send`).
pub struct Monitor {
    token: Retained<AnyObject>,
    _handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>,
    coalescing: Option<bool>,
}

impl Monitor {
    /// Install the monitor. `callback` gets `Some(sample)` for pen events and `None` when a mouse
    /// moves; it runs on the main thread. Call on the main thread (before or after the event loop
    /// starts).
    pub fn install(callback: impl Fn(Option<Sample>) + 'static) -> Result<Self, Error> {
        if MainThreadMarker::new().is_none() {
            return Err(Error::NotMainThread);
        }
        let state = RefCell::new(State::default());
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit calls a local monitor's handler with the event it is about to
            // dispatch, a valid `NSEvent` that it keeps alive for the duration of the call; we
            // only borrow it within the call.
            let e: &NSEvent = unsafe { event.as_ref() };
            let raw = read(e);
            // The handler can't re-enter itself (AppKit runs it synchronously on the main
            // thread), but `try_borrow_mut` keeps a re-entrant call from panicking anyway.
            if let Ok(mut st) = state.try_borrow_mut()
                && let Update::Set(sample) = st.handle(&raw)
            {
                deliver(&callback, sample);
            }
            // Pass the event on unchanged.
            event.as_ptr()
        });
        let mask = NSEventMask(crate::appkit::event_mask());
        // SAFETY: the handler returns the (non-null, valid) event it was given, as the API
        // requires ("block's return must be a valid pointer or null"). We keep the block alive
        // in `Monitor` as well, although AppKit copies it.
        let token = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
        let token = token.ok_or_else(|| Error::Platform("addLocalMonitorForEventsMatchingMask returned nil".into()))?;
        Ok(Self { token, _handler: handler, coalescing: None })
    }

    /// Experimental higher-detail mouse/trackpad input. Absolute OS cursor positions and
    /// acceleration are unchanged. AppKit's previous setting is restored with the monitor.
    pub fn disable_mouse_coalescing(&mut self) {
        self.coalescing.get_or_insert_with(NSEvent::isMouseCoalescingEnabled);
        NSEvent::setMouseCoalescingEnabled(false);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if let Some(previous) = self.coalescing {
            NSEvent::setMouseCoalescingEnabled(previous);
        }
        // SAFETY: `token` is exactly the object `addLocalMonitorForEventsMatchingMask:handler:`
        // returned, removed once (here). `Monitor` is not `Send`, so this runs on the main thread.
        unsafe { NSEvent::removeMonitor(&self.token) };
    }
}

/// Read the tablet fields of an event. Every getter is safe for every event type: AppKit
/// returns 0 / zero points for fields an event doesn't carry (`subtype` and the proximity fields
/// are only read for the event types that define them, see [`crate::appkit`]).
fn read(e: &NSEvent) -> RawEvent {
    let kind = e.r#type().0;
    let tabletish = matches!(kind, crate::appkit::event_type::TABLET_POINT | crate::appkit::event_type::TABLET_PROXIMITY);
    let mouse = !tabletish && is_mouse(kind);
    // `subtype` raises an exception for event types without one; ask only mouse events.
    let subtype = if mouse { e.subtype().0 } else { 0 };
    let proximity = kind == crate::appkit::event_type::TABLET_PROXIMITY || (mouse && subtype == crate::appkit::subtype::TABLET_PROXIMITY);
    let point = kind == crate::appkit::event_type::TABLET_POINT || (mouse && subtype == crate::appkit::subtype::TABLET_POINT);
    let (pressure, tilt, rotation) = if point {
        let t = e.tilt();
        (e.pressure(), (t.x, t.y), e.rotation())
    } else {
        (0.0, (0.0, 0.0), 0.0)
    };
    let (device, entering) = if proximity { (e.pointingDeviceType().0, e.isEnteringProximity()) } else { (0, false) };
    RawEvent { kind, subtype, pressure, tilt, rotation, device, entering }
}

fn is_mouse(kind: usize) -> bool {
    use crate::appkit::event_type::*;
    matches!(
        kind,
        LEFT_MOUSE_DOWN
            | LEFT_MOUSE_UP
            | RIGHT_MOUSE_DOWN
            | RIGHT_MOUSE_UP
            | MOUSE_MOVED
            | LEFT_MOUSE_DRAGGED
            | RIGHT_MOUSE_DRAGGED
            | OTHER_MOUSE_DOWN
            | OTHER_MOUSE_UP
            | OTHER_MOUSE_DRAGGED
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_graphics::{CGEvent, CGEventField, CGEventMouseSubtype, CGEventType, CGMouseButton};
    use objc2_foundation::NSPoint;

    #[test]
    fn install_off_the_main_thread_is_an_error() {
        // `cargo test` runs tests on worker threads, never on the main thread.
        let r = std::thread::spawn(|| Monitor::install(|_| {}).err()).join().unwrap();
        assert_eq!(r, Some(Error::NotMainThread));
    }

    /// A real `NSEvent` built from a Quartz event, as the tablet driver posts them.
    fn event(kind: CGEventType, set: impl Fn(&CGEvent)) -> objc2::rc::Retained<NSEvent> {
        // Quartz builds mouse events only; other types start blank and get their type set.
        let cg = CGEvent::new_mouse_event(None, kind, NSPoint::new(10.0, 10.0), CGMouseButton::Left)
            .or_else(|| {
                let e = CGEvent::new(None)?;
                CGEvent::set_type(Some(&e), kind);
                Some(e)
            })
            .expect("CGEvent");
        set(&cg);
        NSEvent::eventWithCGEvent(&cg).expect("NSEvent")
    }

    fn int(e: &CGEvent, f: CGEventField, v: i64) {
        CGEvent::set_integer_value_field(Some(e), f, v);
    }

    fn dbl(e: &CGEvent, f: CGEventField, v: f64) {
        CGEvent::set_double_value_field(Some(e), f, v);
    }

    /// Tablet mouse events read back through the same path as live ones.
    #[test]
    fn tablet_events_read_through_the_monitor_path() {
        let mut st = State::default();
        let drag = event(CGEventType::LeftMouseDragged, |e| {
            int(e, CGEventField::MouseEventSubtype, i64::from(CGEventMouseSubtype::TabletPoint.0));
            dbl(e, CGEventField::MouseEventPressure, 0.25);
            dbl(e, CGEventField::TabletEventPointPressure, 0.25);
            dbl(e, CGEventField::TabletEventTiltX, 0.5);
            dbl(e, CGEventField::TabletEventTiltY, -0.5);
            dbl(e, CGEventField::TabletEventRotation, 45.0);
        });
        let raw = read(&drag);
        assert_eq!((raw.kind, raw.subtype), (6, 1));
        let Update::Set(Some(s)) = st.handle(&raw) else { panic!("{raw:?}") };
        assert!((s.pressure - 0.25).abs() < 0.01, "{s:?}"); // Quartz keeps 8 bits of mouse pressure.
        assert!((s.tilt_x - 30.0).abs() < 0.1 && (s.tilt_y - 30.0).abs() < 0.1, "{s:?}");
        assert!((s.rotation - 45.0).abs() < 0.1, "{s:?}");
        assert!(!s.eraser);

        // Eraser end entering proximity, then a press with it.
        let prox = event(CGEventType::TabletProximity, |e| {
            int(e, CGEventField::TabletProximityEventPointerType, crate::appkit::device::ERASER as i64);
            int(e, CGEventField::TabletProximityEventEnterProximity, 1);
        });
        let raw = read(&prox);
        assert_eq!((raw.kind, raw.device, raw.entering), (24, 3, true));
        assert!(matches!(st.handle(&raw), Update::Set(Some(Sample { eraser: true, .. }))));

        // A plain mouse event (subtype 0) is a mouse, whatever its pressure.
        let mouse = event(CGEventType::LeftMouseDown, |e| dbl(e, CGEventField::MouseEventPressure, 0.7));
        let raw = read(&mouse);
        assert_eq!((raw.kind, raw.subtype), (1, 0));
        assert_eq!(st.handle(&raw), Update::Set(None));
    }
}

/// Separate native diagnostics window; never modifies the user's PhotoCraft document.
#[cfg(feature = "input-probe")]
pub use probe::run_input_probe;

#[cfg(feature = "input-probe")]
mod probe {
    use super::*;
    use crate::input_probe::{self, Capture, Sample as ProbeSample};
    use objc2::runtime::ProtocolObject;
    use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBezierPath, NSButton, NSColor, NSGraphicsContext, NSTextField, NSTouchPhase,
        NSTouchTypeMask, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    };
    use objc2_foundation::{NSNotification, NSObjectProtocol, NSPoint, NSProcessInfo, NSRect, NSSize, NSString};
    use objc2_game_controller::{GCDevice, GCMouse};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Instant,
    };

    const WIDTH: f64 = 360.0;
    const HEIGHT: f64 = 540.0;
    const TOP: f64 = 135.0;

    struct Data {
        capture: Capture,
        start: Instant,
        uptime: f64,
        active: bool,
        focused: bool,
        replay: bool,
        stroke: u32,
        painting: bool,
        origin: f64,
        output: PathBuf,
        gc: input_probe::Alignment,
        touch: input_probe::Alignment,
    }
    impl Data {
        fn push(&mut self, mut s: ProbeSample) {
            if !self.active || (s.source == "gc" && !self.focused) || !s.time.is_finite() || s.point.iter().any(|p| !p.is_finite() || p.abs() > 1e7) {
                return;
            }
            if self.capture.samples.len() >= input_probe::LIMIT {
                self.active = false;
                self.painting = false;
                self.capture.stopped_reason = "50,000 sample limit".into();
                return;
            }
            s.stroke = if self.painting { self.stroke } else { 0 };
            self.capture.samples.push(s);
        }
    }
    fn write_snapshot(capture: &Capture, output: &std::path::Path) -> Result<(input_probe::Alignment, input_probe::Alignment), String> {
        let gc_calibration = input_probe::calibrate(capture, "gc");
        let touch_calibration = input_probe::calibrate(capture, "touch");
        let gc = input_probe::align_with_shift(capture, "gc", gc_calibration.as_ref().map_or(0, |c| c.lag_ms));
        let touch = input_probe::align_with_shift(capture, "touch", touch_calibration.as_ref().map_or(0, |c| c.lag_ms));
        let bytes = serde_json::to_vec_pretty(capture).map_err(|e| e.to_string())?;
        std::fs::write(output, bytes).map_err(|e| e.to_string())?;
        let report = serde_json::json!({"streams":input_probe::reports(capture), "gc":&gc, "touch":&touch,
                "gc_calibration": gc_calibration,
                "touch_calibration": touch_calibration,
                "note":"Retrospective endpoint matching; receipt-time alignment is uncertain. Extra points are estimated, not OS positions."});
        std::fs::write(output.with_extension("report.json"), serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok((gc, touch))
    }
    type Shared = Arc<Mutex<Data>>;
    struct Ivars {
        data: Shared,
        devices: RefCell<Vec<Retained<GCMouse>>>,
        status: RefCell<Option<Retained<NSTextField>>>,
        original_coalescing: bool,
    }
    define_class!(
        // SAFETY: NSView subclass is main-thread only, contains no borrowed ObjC data,
        // and every override below has the documented NSResponder/NSView signature.
        #[unsafe(super = NSView)]
        #[thread_kind = MainThreadOnly]
        #[ivars = Ivars]
        struct ProbeView;
        unsafe impl NSObjectProtocol for ProbeView {}
        impl ProbeView {
            #[unsafe(method(isFlipped))]
            fn flipped(&self) -> bool { true }
            #[unsafe(method(acceptsFirstResponder))]
            fn first_responder(&self) -> bool { true }
            #[unsafe(method(drawRect:))]
            fn draw(&self, _rect: NSRect) { self.paint(); }
            #[unsafe(method(mouseDown:))]
            fn down(&self, event: &NSEvent) { self.pointer(event, "down"); }
            #[unsafe(method(mouseDragged:))]
            fn dragged(&self, event: &NSEvent) { self.pointer(event, "move"); }
            #[unsafe(method(mouseMoved:))]
            fn moved(&self, event: &NSEvent) { self.pointer(event, "hover"); }
            #[unsafe(method(mouseUp:))]
            fn up(&self, event: &NSEvent) { self.pointer(event, "up"); }
            #[unsafe(method(touchesBeganWithEvent:))]
            fn touches_began(&self, event: &NSEvent) { self.touches(event); }
            #[unsafe(method(touchesMovedWithEvent:))]
            fn touches_moved(&self, event: &NSEvent) { self.touches(event); }
            #[unsafe(method(touchesEndedWithEvent:))]
            fn touches_ended(&self, event: &NSEvent) { self.touches(event); }
            #[unsafe(method(touchesCancelledWithEvent:))]
            fn touches_cancelled(&self, event: &NSEvent) { self.touches(event); }
            #[unsafe(method(toggle:))]
            fn toggle(&self, _sender: &NSButton) {
                let mut d=lock(&self.ivars().data); d.painting=false;
                if d.replay || d.capture.coalescing!=NSEvent::isMouseCoalescingEnabled() {
                    d.capture.stopped_reason="Click New capture before recording a different configuration".into();
                    drop(d); self.refresh(); return;
                }
                d.active = !d.active;
                d.capture.stopped_reason=if d.active { String::new() } else { "Paused by user".into() }; drop(d); self.save();
            }
            #[unsafe(method(save:))]
            fn save_action(&self, _sender: &NSButton) { self.save(); }
            #[unsafe(method(clear:))]
            fn clear(&self, _sender: &NSButton) {
                if !self.save() {return;}
                let mut d=lock(&self.ivars().data); d.replay=false;
                d.capture.samples.clear(); d.capture.coalescing=NSEvent::isMouseCoalescingEnabled(); d.stroke=0; d.painting=false; d.active=true;
                d.capture.stopped_reason.clear(); d.output=output_path(&d.output);
                d.gc=input_probe::Alignment::default(); d.touch=input_probe::Alignment::default();
                drop(d); self.refresh(); self.setNeedsDisplay(true);
            }
            #[unsafe(method(coalescing:))]
            fn coalescing(&self, _sender: &NSButton) {
                if !self.save() {return;}
                let enabled=!NSEvent::isMouseCoalescingEnabled(); NSEvent::setMouseCoalescingEnabled(enabled);
                let mut d=lock(&self.ivars().data); d.painting=false; d.active=false;
                d.capture.stopped_reason="Coalescing changed; click New capture to start a separate run".into();
                drop(d); self.refresh();
            }
        }
        // SAFETY: Delegate signatures match AppKit; window retains this view as its content.
        unsafe impl NSWindowDelegate for ProbeView {
            #[unsafe(method(windowDidBecomeKey:))]
            fn became_key(&self, _notification: &NSNotification) { lock(&self.ivars().data).focused=true; }
            #[unsafe(method(windowDidResignKey:))]
            fn resign(&self, _notification: &NSNotification) {
                {let mut d=lock(&self.ivars().data); d.painting=false; d.focused=false;} self.save();
            }
            #[unsafe(method(windowWillClose:))]
            fn close(&self, _notification: &NSNotification) {
                self.save(); NSEvent::setMouseCoalescingEnabled(self.ivars().original_coalescing);
                // SAFETY: Main-thread application termination, no borrowed ObjC pointers escape.
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
        }
    );
    fn lock(data: &Shared) -> std::sync::MutexGuard<'_, Data> {
        data.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn output_path(previous: &std::path::Path) -> PathBuf {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        previous.parent().unwrap_or(std::path::Path::new(".")).join(format!("input-{stamp}.json"))
    }
    fn sample(source: &str, data: &Data, point: [f64; 2], phase: &str, event: Option<&NSEvent>) -> ProbeSample {
        ProbeSample {
            source: source.into(),
            time: data.start.elapsed().as_secs_f64(),
            event_time: event.map(|e| e.timestamp() - data.uptime),
            profile_time: None,
            stroke: 0,
            device: 0,
            identity: 0,
            phase: phase.into(),
            point,
            resting: false,
            contacts: 0,
        }
    }
    impl ProbeView {
        fn pointer(&self, event: &NSEvent, phase: &str) {
            self.install_devices();
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            let mut d = lock(&self.ivars().data);
            if phase == "down" {
                if !d.active || !(TOP..TOP + HEIGHT).contains(&p.y) {
                    return;
                }
                let column = ((p.x - 20.0) / (WIDTH + 20.0)).floor();
                if !(0.0..3.0).contains(&column) {
                    return;
                }
                d.origin = 20.0 + column * (WIDTH + 20.0);
                if p.x - d.origin > WIDTH {
                    return;
                }
                d.stroke = d.stroke.saturating_add(1);
                d.painting = true;
            }
            let point = [p.x - d.origin, p.y - TOP];
            let s = sample("os", &d, point, phase, Some(event));
            d.push(s);
            let end = phase == "up";
            if end {
                d.painting = false;
            }
            drop(d);
            if end {
                self.save();
            } else if phase != "hover" {
                self.setNeedsDisplay(true);
            }
        }
        fn touches(&self, event: &NSEvent) {
            let touches = event.touchesMatchingPhase_inView(NSTouchPhase::Any, Some(self));
            let contacts = touches.iter().filter(|t| !t.isResting() && t.phase().intersects(NSTouchPhase::Touching)).count();
            let mut d = lock(&self.ivars().data);
            let receipt = d.start.elapsed().as_secs_f64();
            for touch in &touches {
                let p = touch.normalizedPosition();
                let size = touch.deviceSize();
                let phase = if touch.phase().contains(NSTouchPhase::Began) {
                    "began"
                } else if touch.phase().contains(NSTouchPhase::Ended) {
                    "ended"
                } else if touch.phase().contains(NSTouchPhase::Cancelled) {
                    "cancelled"
                } else if touch.phase().contains(NSTouchPhase::Moved) {
                    "moved"
                } else {
                    "stationary"
                };
                let mut s = sample("touch", &d, [p.x * size.width, -p.y * size.height], phase, Some(event));
                // NSObject hash is stable for the identity's life, unlike NSTouch object addresses.
                // SAFETY: Apple documents identity/device as NSObject-compatible objects.
                s.time = receipt;
                s.identity = unsafe { msg_send![&*touch.identity(), hash] };
                s.device = touch.device().map_or(0, |dev| unsafe { msg_send![&*dev, hash] });
                s.resting = touch.isResting();
                s.contacts = contacts;
                d.push(s);
                // SDK: coalescedTouchesForTouch is ONLY valid for DirectTouch, not trackpad Gesture events.
                if event.r#type().0 == 37 {
                    for aux in &event.coalescedTouchesForTouch(&touch) {
                        let p = aux.normalizedPosition();
                        let s = sample("direct-coalesced", &d, [p.x, p.y], phase, Some(event));
                        d.push(s);
                    }
                }
            }
        }
        fn install_devices(&self) {
            // SAFETY: Framework's retained array is valid. Callback block is copied by GCMouseInput.
            // The callback owns only Arc<Mutex<Data>>, never NSView or main-thread-only objects.
            unsafe {
                let mut installed = self.ivars().devices.borrow_mut();
                for mouse in &GCMouse::mice() {
                    if installed.iter().any(|m| std::ptr::eq(&**m, &*mouse)) {
                        continue;
                    }
                    let Some(input) = mouse.mouseInput() else { continue };
                    let id = installed.len() as u64 + 1;
                    let shared = self.ivars().data.clone();
                    mouse.setHandlerQueue(&dispatch2::DispatchQueue::new("PhotoCraft.input-probe", None));
                    let handler = RcBlock::new(move |_input: NonNull<objc2_game_controller::GCMouseInput>, dx: f32, dy: f32| {
                        let received = Instant::now();
                        // SAFETY: Callback's input object is valid throughout this invocation.
                        let profile_time = _input.as_ref().lastEventTimestamp();
                        let mut d = lock(&shared);
                        let mut s = sample("gc", &d, [f64::from(dx), -f64::from(dy)], "delta", None);
                        s.device = id;
                        s.profile_time = profile_time.is_finite().then_some(profile_time);
                        s.time = received.checked_duration_since(d.start).map_or(0.0, |t| t.as_secs_f64());
                        d.push(s);
                    });
                    input.setMouseMovedHandler(RcBlock::as_ptr(&handler));
                    installed.push(mouse);
                }
                lock(&self.ivars().data).capture.gc_devices = installed.len();
            }
        }
        fn save(&self) -> bool {
            let (capture, output) = {
                let d = lock(&self.ivars().data);
                (d.capture.clone(), d.output.clone())
            };
            // Never hold the callback mutex during calibration, graphics or disk writes.
            let result = write_snapshot(&capture, &output);
            if let Err(e) = &result {
                self.set_status(&format!("Save failed: {e}"));
            } else if let Ok((gc, touch)) = &result {
                let mut d = lock(&self.ivars().data);
                d.gc = gc.clone();
                d.touch = touch.clone();
                drop(d);
                self.refresh();
            }
            self.setNeedsDisplay(true);
            result.is_ok()
        }
        fn set_status(&self, text: &str) {
            if let Some(label) = self.ivars().status.borrow().as_ref() {
                label.setStringValue(&NSString::from_str(text));
            }
        }
        fn refresh(&self) {
            let d = lock(&self.ivars().data);
            let streams = input_probe::reports(&d.capture);
            let counts = streams
                .iter()
                .map(|s| format!("{}: {} samples, {:.0} callbacks/s", s.source, s.samples, s.callbacks_per_second))
                .collect::<Vec<_>>()
                .join(" · ");
            self.set_status(&format!(
                "{} · {} · {} GC devices\nEstimated extra points — GC: {} (shift {} ms), touches: {} (shift {} ms). {}\nSaved: {}",
                if d.active { "Recording" } else { "Paused" },
                counts,
                d.capture.gc_devices,
                d.gc.extra_points,
                d.gc.receipt_shift_ms,
                d.touch.extra_points,
                d.touch.receipt_shift_ms,
                d.capture.stopped_reason,
                d.output.display()
            ));
        }
        fn paint(&self) {
            let (os, gc, touch) = {
                let d = lock(&self.ivars().data);
                let mut os = std::collections::BTreeMap::<u32, Vec<[f64; 2]>>::new();
                for s in &d.capture.samples {
                    if s.source == "os" && s.stroke > 0 {
                        os.entry(s.stroke).or_default().push(s.point);
                    }
                }
                (os, d.gc.paths.clone(), d.touch.paths.clone())
            };
            NSColor::windowBackgroundColor().set();
            NSBezierPath::fillRect(self.bounds());
            for column in 0..3 {
                let x = 20.0 + f64::from(column) * (WIDTH + 20.0);
                let rect = NSRect::new(NSPoint::new(x, TOP), NSSize::new(WIDTH, HEIGHT));
                NSColor::whiteColor().set();
                NSBezierPath::fillRect(rect);
                // SAFETY: Balanced graphics state around clipping this panel only.
                NSGraphicsContext::saveGraphicsState_class();
                NSBezierPath::bezierPathWithRect(rect).addClip();
                NSColor::grayColor().set();
                for path in os.values() {
                    draw_path(path, x);
                }
                let paths = if column == 1 { &gc } else { &touch };
                if column > 0 {
                    if column == 1 {
                        NSColor::systemBlueColor().set();
                    } else {
                        NSColor::systemRedColor().set();
                    }
                    for path in paths {
                        draw_path(path, x);
                    }
                } else {
                    NSColor::blackColor().set();
                    for path in os.values() {
                        draw_path(path, x);
                    }
                }
                // SAFETY: Matches saveGraphicsState above.
                NSGraphicsContext::restoreGraphicsState_class();
            }
        }
    }
    fn draw_path(points: &[[f64; 2]], x: f64) {
        let path = NSBezierPath::bezierPath();
        path.setLineWidth(2.0);
        for (i, p) in points.iter().enumerate() {
            let p = NSPoint::new(x + p[0], TOP + p[1]);
            if i == 0 {
                path.moveToPoint(p);
            } else {
                path.lineToPoint(p);
            }
        }
        path.stroke();
    }
    pub fn run_input_probe(directory: PathBuf, initial: Option<Capture>) -> Result<(), Error> {
        let mtm = MainThreadMarker::new().ok_or(Error::NotMainThread)?;
        std::fs::create_dir_all(&directory).map_err(|e| Error::Platform(e.to_string()))?;
        let original = NSEvent::isMouseCoalescingEnabled();
        let active = initial.is_none();
        let capture = initial.unwrap_or_else(|| Capture { coalescing: original, ..Default::default() });
        let gc = input_probe::align_with_shift(&capture, "gc", input_probe::calibrate(&capture, "gc").map_or(0, |c| c.lag_ms));
        let touch = input_probe::align_with_shift(&capture, "touch", input_probe::calibrate(&capture, "touch").map_or(0, |c| c.lag_ms));
        let stroke = capture.samples.iter().map(|s| s.stroke).max().unwrap_or(0);
        let data = Arc::new(Mutex::new(Data {
            capture,
            start: Instant::now(),
            uptime: NSProcessInfo::processInfo().systemUptime(),
            active,
            focused: false,
            replay: !active,
            stroke,
            painting: false,
            origin: 20.0,
            output: output_path(&directory.join("capture.json")),
            gc,
            touch,
        }));
        let view =
            ProbeView::alloc(mtm).set_ivars(Ivars { data, devices: RefCell::new(Vec::new()), status: RefCell::new(None), original_coalescing: original });
        // SAFETY: Correct NSView initializer; fully owned ivars, no borrowed native data.
        let view: Retained<ProbeView> = unsafe { msg_send![super(view),initWithFrame:NSRect::new(NSPoint::new(0.0,0.0),NSSize::new(1160.0,700.0))] };
        view.setAllowedTouchTypes(NSTouchTypeMask::Indirect);
        view.setWantsRestingTouches(true);
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        // SAFETY: NSWindow retained below, auto-release on close disabled. All objects on main thread.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                view.bounds(),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        window.setTitle(&NSString::from_str("PhotoCraft — raw input experiment"));
        window.setContentView(Some(&view));
        window.setDelegate(Some(ProtocolObject::from_ref(&*view)));
        window.setAcceptsMouseMovedEvents(true);
        for (i, (title, action)) in
            [("Pause / resume", sel!(toggle:)), ("Save", sel!(save:)), ("New capture", sel!(clear:)), ("Toggle coalescing", sel!(coalescing:))]
                .into_iter()
                .enumerate()
        {
            // SAFETY: Targets/selectors are the ProbeView methods declared above; view outlives buttons.
            unsafe {
                let button = NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(&view), Some(action), mtm);
                button.setFrame(NSRect::new(NSPoint::new(20.0 + i as f64 * 155.0, 8.0), NSSize::new(150.0, 28.0)));
                view.addSubview(&button);
            }
        }
        let status = NSTextField::labelWithString(&NSString::from_str("Click and scribble in any panel. All streams are captured together."), mtm);
        status.setFrame(NSRect::new(NSPoint::new(20.0, 42.0), NSSize::new(1120.0, 65.0)));
        // SAFETY: Owned labels are retained by the view.
        view.addSubview(&status);
        *view.ivars().status.borrow_mut() = Some(status);
        for (i, title) in ["OS cursor positions", "GC motion aligned to OS endpoints", "Trackpad touches aligned to OS endpoints"].into_iter().enumerate() {
            let label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
            label.setFrame(NSRect::new(NSPoint::new(20.0 + i as f64 * (WIDTH + 20.0), 110.0), NSSize::new(WIDTH, 22.0)));
            view.addSubview(&label);
        }
        let hint = NSTextField::labelWithString(
            &NSString::from_str("Draw in any panel; release to compare. Grey lines are the OS reference. Coloured paths are retrospective estimates."),
            mtm,
        );
        hint.setFrame(NSRect::new(NSPoint::new(20.0, 680.0), NSSize::new(1120.0, 20.0)));
        view.addSubview(&hint);
        view.install_devices();
        view.refresh();
        window.center();
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&view));
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        app.run();
        NSEvent::setMouseCoalescingEnabled(original);
        Ok(())
    }
}
