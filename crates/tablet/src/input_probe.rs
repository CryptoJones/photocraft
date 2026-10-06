//! Opt-in input experiment, not a production brush engine. Raw captures stay unchanged;
//! endpoint-constrained paths are retrospective estimates, never extra measured OS positions.
use serde::{Deserialize, Serialize};

pub const LIMIT: usize = 50_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub source: String,
    /// Monotonic callback receipt time (seconds), shared by all streams.
    pub time: f64,
    /// AppKit event timestamp relative to capture start. GC has no supplied timestamp.
    pub event_time: Option<f64>,
    /// GC profile's latest-update timestamp, not guaranteed to identify this queued delta.
    #[serde(default)]
    pub profile_time: Option<f64>,
    pub stroke: u32,
    pub device: u64,
    pub identity: u64,
    pub phase: String,
    pub point: [f64; 2],
    pub resting: bool,
    pub contacts: usize,
}

impl Sample {
    fn clock(&self) -> f64 {
        if self.source == "os" || self.source == "touch" { self.event_time.unwrap_or(self.time) } else { self.time }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {
    pub version: u8,
    pub samples: Vec<Sample>,
    pub coalescing: bool,
    pub gc_devices: usize,
    pub stopped_reason: String,
}

impl Default for Capture {
    fn default() -> Self {
        Self { version: 1, samples: Vec::new(), coalescing: true, gc_devices: 0, stopped_reason: String::new() }
    }
}

impl Capture {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("Capture exceeds 32 MiB".into());
        }
        let capture: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if capture.version != 1
            || capture.samples.len() > LIMIT
            || capture.samples.iter().any(|s| {
                !s.time.is_finite()
                    || s.time < 0.0
                    || s.event_time.is_some_and(|t| !t.is_finite())
                    || s.profile_time.is_some_and(|t| !t.is_finite())
                    || s.point.iter().any(|p| !p.is_finite() || p.abs() > 1e7)
                    || !["os", "gc", "touch", "direct-coalesced"].contains(&s.source.as_str())
            })
        {
            return Err("Invalid input capture".into());
        }
        Ok(capture)
    }
}

#[derive(Debug, Serialize)]
pub struct StreamReport {
    pub source: String,
    pub samples: usize,
    pub painting_samples: usize,
    pub painting_median_gap_ms: f64,
    pub painting_event_median_gap_ms: Option<f64>,
    pub callbacks_per_second: f64,
    pub median_gap_ms: f64,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct Alignment {
    pub source: String,
    pub receipt_shift_ms: i32,
    pub paths: Vec<Vec<[f64; 2]>>,
    pub extra_points: usize,
    pub accepted_intervals: usize,
    pub fallback_intervals: usize,
    pub min_gain: Option<f64>,
    pub max_gain: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Calibration {
    pub lag_ms: i32,
    pub gain: f64,
    pub rotation_degrees: f64,
    pub held_out_rmse_pixels: f64,
    pub training_intervals: usize,
    pub held_out_intervals: usize,
}

/// Fit a single gain/rotation on alternating intervals, then score untouched intervals.
/// Searching receipt-time shifts diagnoses delivery skew. It cannot recover device timestamps
/// or reproduce the OS's speed-dependent acceleration, and is not used to force preview fits.
pub fn calibrate(capture: &Capture, source: &str) -> Option<Calibration> {
    let mut groups = std::collections::BTreeMap::<u32, Vec<&Sample>>::new();
    for s in &capture.samples {
        if s.stroke > 0 {
            groups.entry(s.stroke).or_default().push(s);
        }
    }
    let mut best: Option<(f64, Calibration)> = None;
    for lag_ms in -20..=20 {
        let mut pairs = Vec::new();
        for group in groups.values() {
            let mut os = group.iter().copied().filter(|s| s.source == "os").collect::<Vec<_>>();
            let mut raw = capture.samples.iter().filter(|s| s.source == source && !s.resting).collect::<Vec<_>>();
            os.sort_by(|a, b| a.clock().total_cmp(&b.clock()));
            raw.sort_by(|a, b| a.clock().total_cmp(&b.clock()));
            for w in os.windows(2) {
                let Some((a, b)) = w.first().zip(w.get(1)) else { continue };
                if b.clock() - a.clock() > 0.05 || b.clock() <= a.clock() {
                    continue;
                }
                let shift = f64::from(lag_ms) / 1000.0;
                let start = raw.partition_point(|s| s.clock() <= a.clock() + shift);
                let end = raw.partition_point(|s| s.clock() <= b.clock() + shift);
                let samples = raw.get(start..end).unwrap_or(&[]);
                let Some(first) = samples.first() else { continue };
                if samples
                    .iter()
                    .any(|s| s.device != first.device || s.identity != first.identity || (source == "touch" && (s.contacts != 1 || s.phase != "moved")))
                {
                    continue;
                }
                let delta = if source == "gc" {
                    samples.iter().fold([0.0; 2], |p, s| [p[0] + s.point[0], p[1] + s.point[1]])
                } else {
                    let Some(previous) = raw.get(..start).unwrap_or(&[]).last().filter(|s| {
                        s.identity == first.identity
                            && s.device == first.device
                            && s.contacts == 1
                            && ["began", "moved"].contains(&s.phase.as_str())
                            && a.clock() + shift - s.clock() <= 0.05
                    }) else {
                        continue;
                    };
                    let Some(last) = samples.last() else { continue };
                    [last.point[0] - previous.point[0], last.point[1] - previous.point[1]]
                };
                if delta[0] * delta[0] + delta[1] * delta[1] > 1e-8 {
                    pairs.push((delta, [b.point[0] - a.point[0], b.point[1] - a.point[1]]));
                }
            }
        }
        let mut norm = 0.0;
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, (raw, screen)) in pairs.iter().enumerate() {
            if i % 2 == 0 {
                norm += raw[0] * raw[0] + raw[1] * raw[1];
                re += raw[0] * screen[0] + raw[1] * screen[1];
                im += raw[0] * screen[1] - raw[1] * screen[0];
            }
        }
        if norm <= 1e-8 || pairs.len() < 10 {
            continue;
        }
        re /= norm;
        im /= norm;
        let mut train = 0.0;
        let mut test = 0.0;
        let mut n_train = 0;
        let mut n_test = 0;
        for (i, (raw, screen)) in pairs.iter().enumerate() {
            let dx = re * raw[0] - im * raw[1] - screen[0];
            let dy = im * raw[0] + re * raw[1] - screen[1];
            if i % 2 == 0 {
                train += dx * dx + dy * dy;
                n_train += 1;
            } else {
                test += dx * dx + dy * dy;
                n_test += 1;
            }
        }
        let score = train / f64::from(n_train);
        let c = Calibration {
            lag_ms,
            gain: re.hypot(im),
            rotation_degrees: im.atan2(re).to_degrees(),
            held_out_rmse_pixels: (test / f64::from(n_test)).sqrt(),
            training_intervals: n_train as usize,
            held_out_intervals: n_test as usize,
        };
        if best.as_ref().is_none_or(|(error, _)| score < *error) {
            best = Some((score, c));
        }
    }
    best.map(|(_, calibration)| calibration)
}

pub fn reports(capture: &Capture) -> Vec<StreamReport> {
    ["os", "gc", "touch", "direct-coalesced"]
        .into_iter()
        .map(|source| {
            let mut times = capture.samples.iter().filter(|s| s.source == source).map(|s| s.time).collect::<Vec<_>>();
            times.sort_by(f64::total_cmp);
            times.dedup(); // All touches in one callback share a receipt time: not separate polling ticks.
            let duration = times.last().zip(times.first()).map_or(0.0, |(a, b)| a - b);
            let mut gaps = times.windows(2).filter_map(|w| w.first().zip(w.get(1)).map(|(a, b)| (b - a) * 1000.0)).collect::<Vec<_>>();
            gaps.sort_by(f64::total_cmp);
            let mut painting_times = capture.samples.iter().filter(|s| s.source == source && s.stroke > 0).map(|s| s.time).collect::<Vec<_>>();
            painting_times.sort_by(f64::total_cmp);
            painting_times.dedup();
            let mut painting_gaps = painting_times
                .windows(2)
                .filter_map(|w| w.first().zip(w.get(1)).map(|(a, b)| (b - a) * 1000.0))
                .filter(|gap| *gap > 0.0 && *gap < 100.0)
                .collect::<Vec<_>>();
            painting_gaps.sort_by(f64::total_cmp);
            let mut event_times = capture.samples.iter().filter(|s| s.source == source && s.stroke > 0).filter_map(|s| s.event_time).collect::<Vec<_>>();
            event_times.sort_by(f64::total_cmp);
            event_times.dedup();
            let mut event_gaps = event_times
                .windows(2)
                .filter_map(|w| w.first().zip(w.get(1)).map(|(a, b)| (b - a) * 1000.0))
                .filter(|gap| *gap > 0.0 && *gap < 100.0)
                .collect::<Vec<_>>();
            event_gaps.sort_by(f64::total_cmp);
            StreamReport {
                source: source.into(),
                samples: capture.samples.iter().filter(|s| s.source == source).count(),
                painting_samples: capture.samples.iter().filter(|s| s.source == source && s.stroke > 0).count(),
                callbacks_per_second: if duration > 0.0 { times.len().saturating_sub(1) as f64 / duration } else { 0.0 },
                median_gap_ms: gaps.get(gaps.len() / 2).copied().unwrap_or(0.0),
                painting_median_gap_ms: painting_gaps.get(painting_gaps.len() / 2).copied().unwrap_or(0.0),
                painting_event_median_gap_ms: event_gaps.get(event_gaps.len() / 2).copied(),
            }
        })
        .collect()
}

/// Map intermediate raw positions between two measured OS positions with a similarity
/// transform. Acceleration gain is estimated per interval. Reject ambiguous fingers/devices,
/// lifts, gaps >50 ms, cancelling net motion and excessive gain. Fall back to OS endpoints.
/// Matching endpoints is a constraint, NOT evidence of correct intermediate coordinates.
pub fn align(capture: &Capture, source: &str) -> Alignment {
    align_with_shift(capture, source, 0)
}

pub fn align_with_shift(capture: &Capture, source: &str, receipt_shift_ms: i32) -> Alignment {
    let receipt_shift_ms = receipt_shift_ms.clamp(-20, 20);
    let shift = f64::from(receipt_shift_ms) / 1000.0;
    let mut result = Alignment { source: source.into(), receipt_shift_ms, ..Default::default() };
    let mut strokes = std::collections::BTreeMap::<u32, Vec<&Sample>>::new();
    for sample in &capture.samples {
        if sample.stroke > 0 {
            strokes.entry(sample.stroke).or_default().push(sample);
        }
    }
    for group in strokes.values() {
        let mut anchors = group.iter().copied().filter(|s| s.source == "os").collect::<Vec<_>>();
        anchors.sort_by(|a, b| a.clock().total_cmp(&b.clock()));
        let mut raw = capture.samples.iter().filter(|s| s.source == source && (source != "touch" || !s.resting)).collect::<Vec<_>>();
        raw.sort_by(|a, b| a.clock().total_cmp(&b.clock()));
        let mut path = Vec::new();
        if let Some(a) = anchors.first() {
            path.push(a.point);
        }
        for pair in anchors.windows(2) {
            let Some((a, b)) = pair.first().zip(pair.get(1)) else { continue };
            let start = raw.partition_point(|s| s.clock() <= a.clock() + shift);
            let end = raw.partition_point(|s| s.clock() <= b.clock() + shift);
            let samples = raw.get(start..end).unwrap_or(&[]);
            let valid = !samples.is_empty()
                && b.clock() - a.clock() <= 0.05
                && samples.iter().all(|s| {
                    !s.resting
                        && (source != "touch" || (s.contacts == 1 && s.phase == "moved"))
                        && samples.first().is_some_and(|first| first.device == s.device && first.identity == s.identity)
                });
            let mut positions = Vec::new();
            let mut current = [0.0; 2];
            if valid {
                if source == "touch" {
                    // A prior touch sample is necessary; never guess a finger's starting point.
                    if let Some(previous) = raw.get(..start).unwrap_or(&[]).last().filter(|s| {
                        !s.resting
                            && s.contacts == 1
                            && samples.first().is_some_and(|first| first.device == s.device && first.identity == s.identity)
                            && a.clock() + shift - s.clock() <= 0.05
                            && ["moved", "began"].contains(&s.phase.as_str())
                    }) {
                        current = previous.point;
                    } else {
                        result.fallback_intervals += 1;
                        path.push(b.point);
                        continue;
                    }
                }
                let origin = current;
                for s in samples {
                    if source == "gc" {
                        current = [current[0] + s.point[0], current[1] + s.point[1]];
                    } else {
                        current = s.point;
                    }
                    positions.push([current[0] - origin[0], current[1] - origin[1]]);
                }
                if let Some(end) = positions.last() {
                    let screen = [b.point[0] - a.point[0], b.point[1] - a.point[1]];
                    let norm = end[0] * end[0] + end[1] * end[1];
                    let gain = if norm > 1e-8 { (screen[0] * screen[0] + screen[1] * screen[1]).sqrt() / norm.sqrt() } else { 0.0 };
                    if norm > 1e-8 && (0.01..=64.0).contains(&gain) {
                        let re = (screen[0] * end[0] + screen[1] * end[1]) / norm;
                        let im = (screen[1] * end[0] - screen[0] * end[1]) / norm;
                        // Last raw position maps to b; include it only once as the OS anchor.
                        for p in positions.iter().take(positions.len().saturating_sub(1)) {
                            path.push([a.point[0] + re * p[0] - im * p[1], a.point[1] + im * p[0] + re * p[1]]);
                            result.extra_points += 1;
                        }
                        result.min_gain = Some(result.min_gain.map_or(gain, |g| g.min(gain)));
                        result.max_gain = Some(result.max_gain.map_or(gain, |g| g.max(gain)));
                        result.accepted_intervals += 1;
                        path.push(b.point);
                        continue;
                    }
                }
            }
            result.fallback_intervals += 1;
            path.push(b.point);
        }
        if !path.is_empty() {
            result.paths.push(path);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(source: &str, t: f64, p: [f64; 2]) -> Sample {
        Sample {
            source: source.into(),
            time: t,
            event_time: None,
            profile_time: None,
            stroke: 1,
            device: 1,
            identity: 1,
            phase: "moved".into(),
            point: p,
            resting: false,
            contacts: 1,
        }
    }
    fn capture() -> Capture {
        Capture {
            samples: vec![sample("os", 0.0, [10.0, 20.0]), sample("gc", 0.01, [1.0, 1.0]), sample("gc", 0.02, [1.0, -1.0]), sample("os", 0.025, [14.0, 20.0])],
            ..Default::default()
        }
    }
    #[test]
    fn extra_points_keep_bends_and_match_os_endpoints() {
        let a = align(&capture(), "gc");
        assert_eq!(a.paths, vec![vec![[10.0, 20.0], [12.0, 22.0], [14.0, 20.0]]]);
        assert_eq!(a.extra_points, 1);
        assert_eq!(a.min_gain, Some(2.0));
    }
    #[test]
    fn ambiguity_gaps_and_cancelling_motion_fall_back() {
        for mode in 0..5 {
            let mut c = capture();
            match mode {
                0 => c.samples[2].device = 2,
                1 => c.samples[3].time = 0.1,
                2 => c.samples[2].point = [-1.0, -1.0],
                3 => c.samples[2].resting = true,
                _ => c.samples[3].point = [10000.0, 20.0],
            }
            let a = align(&c, "gc");
            assert_eq!(a.extra_points, 0);
            assert_eq!(a.fallback_intervals, 1);
        }
    }
    #[test]
    fn finger_lifts_and_multiple_contacts_never_bridge() {
        let mut c = capture();
        c.samples = vec![
            sample("touch", 0.0, [0.0, 0.0]),
            sample("os", 0.001, [10.0, 20.0]),
            sample("touch", 0.01, [1.0, 1.0]),
            sample("touch", 0.02, [2.0, 0.0]),
            sample("os", 0.025, [14.0, 20.0]),
        ];
        assert_eq!(align(&c, "touch").extra_points, 1);
        for mode in 0..3 {
            let mut bad = c.clone();
            match mode {
                0 => bad.samples[2].identity = 2,
                1 => bad.samples[2].contacts = 2,
                _ => bad.samples[2].phase = "ended".into(),
            }
            assert_eq!(align(&bad, "touch").extra_points, 0);
        }
    }
    #[test]
    fn touch_contacts_are_not_independent_polling_ticks() {
        let c =
            Capture { samples: vec![sample("touch", 1.0, [0.0; 2]), sample("touch", 1.0, [0.0; 2]), sample("touch", 1.01, [0.0; 2])], ..Default::default() };
        let r = reports(&c);
        assert!((r[2].callbacks_per_second - 100.0).abs() < 1e-5);
    }
    #[test]
    fn calibration_scores_unforced_held_out_positions() {
        let mut c = Capture::default();
        c.samples.push(sample("os", 0.0, [0.0, 0.0]));
        for i in 1..=20 {
            let t = f64::from(i) * 0.01;
            c.samples.push(sample("gc", t - 0.002, [1.0, 0.0]));
            c.samples.push(sample("os", t, [f64::from(i) * 2.0, 0.0]));
        }
        let audit = calibrate(&c, "gc").unwrap();
        assert!((audit.gain - 2.0).abs() < 1e-6);
        assert!(audit.held_out_rmse_pixels < 1e-6);
        assert!(audit.held_out_intervals >= 5);
        // Variable OS acceleration breaks a single global gain even though endpoint-fitted
        // previews still match their anchors exactly.
        for (i, s) in c.samples.iter_mut().filter(|s| s.source == "os").enumerate() {
            s.point[0] += if i % 2 == 0 { 3.0 } else { 0.0 };
        }
        assert!(calibrate(&c, "gc").unwrap().held_out_rmse_pixels > 1.0);
    }

    #[test]
    fn receipt_shift_does_not_change_os_anchor_positions() {
        let mut c = capture();
        for s in c.samples.iter_mut().filter(|s| s.source == "gc") {
            s.time += 0.01;
        }
        let a = align_with_shift(&c, "gc", 10);
        assert_eq!(a.paths, vec![vec![[10.0, 20.0], [12.0, 22.0], [14.0, 20.0]]]);
        assert_eq!(a.receipt_shift_ms, 10);
    }

    #[test]
    fn queued_appkit_delivery_uses_event_clock_not_burst_receipts() {
        let mut c = capture();
        for s in c.samples.iter_mut().filter(|s| s.source == "os") {
            s.event_time = Some(s.time);
            s.time += 0.2;
        }
        assert_eq!(align(&c, "gc").extra_points, 1);
        // Independent raw callbacks arriving before the queued button dispatch are captured
        // as hover, but their timestamps still place them inside the physical stroke.
        for s in c.samples.iter_mut().filter(|s| s.source == "gc") {
            s.stroke = 0;
        }
        assert_eq!(align(&c, "gc").extra_points, 1);
    }

    #[test]
    fn bounded_reload_and_empty_capture() {
        assert!(Capture::load(b"{}").is_err());
        let c = capture();
        assert!(Capture::load(&serde_json::to_vec(&c).unwrap()).is_ok());
        assert_eq!(align(&Capture::default(), "gc").extra_points, 0);
    }
}
