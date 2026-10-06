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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_roundtrip_and_version_validation() {
        let mut capture=Capture::default();
        assert!(Capture::load(&serde_json::to_vec(&capture).unwrap()).is_ok());
        capture.version=99;
        assert!(Capture::load(&serde_json::to_vec(&capture).unwrap()).is_err());
        assert!(Capture::load(b"{}").is_err());
    }
}
