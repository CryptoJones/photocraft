# Native motion capture experiment

This separate Rust/AppKit window records **all available streams together** while you click
and draw. It does not change PhotoCraft documents or the production brush engine.

```sh
cargo run -p photocraft-tablet --features input-probe --example input_probe
```

Draw in any of the three panels and release to compare. Left: OS cursor positions. Middle:
GC deltas fitted between OS positions. Right: trackpad finger movement fitted between OS
positions. Grey lines in the latter panels are the OS reference. There is no smoothing.
Coloured paths are **retrospective estimates**, not additional measured cursor positions.
Each release saves a uniquely named JSON capture and report in `plan/evidence/raw-input/`.
Pause/resume, Save, and New capture are explicit controls. New capture saves the previous run.
Toggle coalescing pauses the run; start a new capture to compare the other AppKit setting.
The original coalescing setting is restored when the probe closes. No server is started.

```sh
cargo run -p photocraft-tablet --features input-probe --example input_probe -- \
  --analyze /absolute/path/input.json
cargo run -p photocraft-tablet --features input-probe --example input_probe -- \
  --replay /absolute/path/input.json
```

## What the APIs supply

- [GCMouse](https://developer.apple.com/documentation/gamecontroller/gcmousemoved) supplies
  unaccelerated relative deltas, not screen coordinates. Device availability and delivery
  rate must be measured. The probe installs a separate serial callback queue for each device.
- [NSTouch](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingTouchEvents/HandlingTouchEvents.html)
  supplies normalized finger positions. `deviceSize` converts these to physical trackpad
  points (72 points/inch); the probe flips Y into canvas orientation. These are not pixels.
- **Correction:** `coalescedTouchesForTouch:` is valid only for `NSEventTypeDirectTouch`, per
  the installed Apple SDK's `NSEvent.h`. Ordinary trackpad touches are indirect gesture
  events; this API cannot recover hidden trackpad samples. The probe never calls it on an
  indirect event. NSTouch also does not expose a per-sample hardware timestamp.
- All streams retain monotonic **callback receipt times**. OS/touch records also retain the
  enclosing NSEvent timestamp separately; fitting uses that event clock for AppKit streams.
  GC callbacks provide no per-delta timestamp argument. The probe also records the profile's
  `lastEventTimestamp` for investigation, but it may refer to a newer update than a queued
  callback; it is not assumed to be a hardware timestamp for that delta. In this local run
  profile timestamps resembled Unix wall time, whereas AppKit used system uptime; some
  successive GC callbacks shared a profile timestamp. Clock origin, clock changes and
  queued state updates must be checked before using those values to align individual deltas.
  Rates
  count unique callback times; simultaneous fingers do not count as separate polling ticks.

## Alignment and limits

The probe fits a timing shift from -20 to +20 ms and a global gain/rotation on alternating
OS intervals, then measures error on the other intervals. That held-out error is meaningful;
forcing reconstructed endpoints to match the OS is not an accuracy test. The preview uses
the fitted receipt shift, then a separate local similarity transform for each pair of OS
anchors to accommodate changing acceleration. This requires the next anchor and therefore
adds latency if used live. It is not yet a production recommendation.

- Finger lifts, cancellation, changes of identity/device, multiple active fingers, resting
  contacts, missing samples, gaps over 50 ms and near-zero net movement need special handling.
  Ambiguous intervals fall back to OS endpoints. Resting touches are recorded but excluded
  from fitting. Loops whose endpoint displacement cancels cannot determine a unique gain.
- A GC delta cannot identify which device produced a particular OS cursor event. Concurrent
  devices, remote-desktop/synthetic events and accessibility drag gestures need separate runs.
  GC captures are focus gated; losing focus ends the current stroke.
- The canvas is fixed at 360 × 540 document coordinates, one logical window point per document
  coordinate. Retina backing pixels do not change those units. A future editor integration
  must freeze the view transform per input interval and split on zoom/pan/rotation/DPI changes.
- Raw originals are saved independently of fitted paths. Capture is capped at 50,000 records;
  reload is capped at 32 MiB. Replay is deterministic and can be reanalyzed without hardware.

## First local capture, 2026-10-06

Six strokes: 605 OS points, 694 GC samples, 1,143 touch records. Median painting gaps were
16.64, 11.30 and 8.34 ms respectively (gaps ≥100 ms excluded). A fitted global transform
left 4.21 document pixels GC and 5.51 pixels touch held-out RMS error. The best clock-alignment shifts
were +6 and -4 ms. These are delivery-alignment estimates, not measured hardware latency.
A subsequent bursty capture had touch event gaps of 8.04 ms but receipt gaps of only 0.44 ms:
queued delivery must not be presented as a 2,000 Hz trackpad. Calibration and graphics now
release the raw-input mutex before doing work, and fitting retains event/receipt clocks.
This proves additional information is available in this run, not that endpoint fitting is
accurate enough to ship. Next evidence: fast curves/reversals, slow straight lines, repeated
finger lifts, thumb clicking, both coalescing settings, and independent mouse/trackpad runs.

Later runs with the fixed collector put median painting delivery near 8.4 ms for both OS
and GC, and trackpad event gaps near 8.0 ms. Additional samples still existed, but global
held-out errors were about 8–10 pixels on some fast scribbles and larger on another run.
This is useful capture/replay evidence, not a demonstrated latency or accuracy win.
