//! Isotonic calibration of the displayed confidence (P4).
//!
//! The arbiter's blended score (`raw`) ranks labels well but is not a
//! probability: a raw 0.8 might be accepted 95% of the time for one person
//! and 60% for another. Calibration maps raw scores onto the accept rate the
//! user actually shows, so a displayed "90%" means about 9 in 10 accepted.
//!
//! - **Fit**: pool-adjacent-violators over `(raw, accepted)` pairs gives the
//!   best monotone step function. Each block's rate is Laplace-smoothed,
//!   `(k + 1) / (n + 2)`, so a handful of outcomes can never promise 100%,
//!   and a second weighted pass restores monotonicity after smoothing.
//! - **Apply**: linear interpolation between block centers, flat beyond the
//!   first and last, so the curve is continuous and never decreasing.
//! - **Cold start**: with no calibrator yet (fewer than `MIN_SAMPLES`
//!   verdicts), displayed confidence is capped at `COLD_START_CAP`.
//!
//! Rule hits are p = 1.0 by definition and are neither calibrated nor used as
//! samples.

use serde::{Deserialize, Serialize};

use super::{COLD_START_CAP, COLD_START_OUTCOMES};

/// Verdicts needed before a calibrator replaces the cold-start cap.
pub const MIN_SAMPLES: usize = COLD_START_OUTCOMES as usize;

/// A fitted calibration curve, stored as JSON in `model_artifacts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibrator {
    /// Knots, raw score to accept rate, both ascending.
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub samples: u32,
}

impl Calibrator {
    /// Fits a curve, or `None` below `MIN_SAMPLES`.
    pub fn fit(samples: &[(f64, bool)]) -> Option<Self> {
        if samples.len() < MIN_SAMPLES {
            return None;
        }
        let mut sorted: Vec<(f64, bool)> = samples
            .iter()
            .filter(|(raw, _)| raw.is_finite())
            .map(|(raw, accepted)| (raw.clamp(0.0, 1.0), *accepted))
            .collect();
        if sorted.len() < MIN_SAMPLES {
            return None;
        }
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));

        // Identical scores start in one block, so ties can't become two knots.
        let mut initial: Vec<Block> = Vec::new();
        let mut previous: Option<f64> = None;
        for (raw, accepted) in &sorted {
            let y = if *accepted { 1.0 } else { 0.0 };
            match initial.last_mut() {
                Some(last) if previous == Some(*raw) => last.add(*raw, y),
                _ => initial.push(Block {
                    sum_x: *raw,
                    sum_y: y,
                    n: 1.0,
                }),
            }
            previous = Some(*raw);
        }

        let smoothed = pav(initial).into_iter().map(|block| Block {
            sum_y: (block.sum_y + 1.0) / (block.n + 2.0) * block.n,
            ..block
        });
        let blocks = pav(smoothed);
        Some(Self {
            x: blocks.iter().map(|b| b.sum_x / b.n).collect(),
            y: blocks.iter().map(|b| b.sum_y / b.n).collect(),
            samples: sorted.len() as u32,
        })
    }

    pub fn apply(&self, raw: f64) -> f64 {
        let (Some(first), Some(last)) = (self.x.first(), self.x.last()) else {
            return raw;
        };
        if raw <= *first {
            return self.y[0];
        }
        if raw >= *last {
            return self.y[self.y.len() - 1];
        }
        let i = self.x.partition_point(|x| *x <= raw);
        let (x0, x1, y0, y1) = (self.x[i - 1], self.x[i], self.y[i - 1], self.y[i]);
        y0 + (y1 - y0) * (raw - x0) / (x1 - x0)
    }
}

/// The confidence the user sees for a model (non-rule) score.
pub fn display(raw: f64, calibrator: Option<&Calibrator>) -> f64 {
    match calibrator {
        Some(calibrator) => calibrator.apply(raw),
        None => raw.min(COLD_START_CAP),
    }
}

#[derive(Debug, Clone, Copy)]
struct Block {
    sum_x: f64,
    sum_y: f64,
    n: f64,
}

impl Block {
    fn add(&mut self, x: f64, y: f64) {
        self.sum_x += x;
        self.sum_y += y;
        self.n += 1.0;
    }

    fn mean(&self) -> f64 {
        self.sum_y / self.n
    }
}

/// Pool adjacent violators: merges neighboring blocks until their means never
/// decrease. Blocks must arrive in ascending `x` order.
fn pav(blocks: impl IntoIterator<Item = Block>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for block in blocks {
        out.push(block);
        while out.len() >= 2 {
            let last = out[out.len() - 1];
            let before = out[out.len() - 2];
            if before.mean() <= last.mean() {
                break;
            }
            out.pop();
            let merged = out.last_mut().expect("two blocks");
            merged.sum_x += last.sum_x;
            merged.sum_y += last.sum_y;
            merged.n += last.n;
        }
    }
    out
}

/// One bucket of the reliability chart: what was promised vs what happened.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReliabilityBin {
    pub lo: f64,
    pub hi: f64,
    pub count: u32,
    /// Mean displayed confidence in the bucket.
    pub predicted: f64,
    /// Share of the bucket the user accepted.
    pub actual: f64,
}

/// Buckets `(shown, accepted)` pairs into `bins` equal-width bins, dropping
/// empty ones. The top bin includes 1.0.
pub fn reliability(samples: &[(f64, bool)], bins: usize) -> Vec<ReliabilityBin> {
    let bins = bins.max(1);
    let mut sums = vec![(0u32, 0.0f64, 0u32); bins];
    for (shown, accepted) in samples {
        let shown = shown.clamp(0.0, 1.0);
        let index = ((shown * bins as f64) as usize).min(bins - 1);
        let bin = &mut sums[index];
        bin.0 += 1;
        bin.1 += shown;
        bin.2 += u32::from(*accepted);
    }
    sums.into_iter()
        .enumerate()
        .filter(|(_, (count, _, _))| *count > 0)
        .map(|(index, (count, shown, accepted))| ReliabilityBin {
            lo: index as f64 / bins as f64,
            hi: (index + 1) as f64 / bins as f64,
            count,
            predicted: shown / count as f64,
            actual: accepted as f64 / count as f64,
        })
        .collect()
}

/// Expected calibration error: the count-weighted gap between promised and
/// actual accept rates. 0 is perfect.
pub fn expected_error(bins: &[ReliabilityBin]) -> Option<f64> {
    let total: u32 = bins.iter().map(|b| b.count).sum();
    (total > 0).then(|| {
        bins.iter()
            .map(|b| f64::from(b.count) * (b.predicted - b.actual).abs())
            .sum::<f64>()
            / f64::from(total)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` samples at `raw`, `accepted` of them accepted.
    fn at(raw: f64, n: usize, accepted: usize) -> Vec<(f64, bool)> {
        (0..n).map(|i| (raw, i < accepted)).collect()
    }

    #[test]
    fn too_few_samples_keep_the_cold_start_cap() {
        assert!(Calibrator::fit(&at(0.9, MIN_SAMPLES - 1, 10)).is_none());
        assert_eq!(display(0.97, None), COLD_START_CAP);
        assert_eq!(display(0.4, None), 0.4);
    }

    #[test]
    fn an_overconfident_model_is_pulled_down_to_its_accept_rate() {
        // Raw 0.9 was accepted 60% of the time, raw 0.6 only 30%.
        let samples = [at(0.9, 100, 60), at(0.6, 100, 30)].concat();
        let c = Calibrator::fit(&samples).unwrap();
        assert!((c.apply(0.9) - 61.0 / 102.0).abs() < 1e-9);
        assert!((c.apply(0.6) - 31.0 / 102.0).abs() < 1e-9);
        // Between knots it interpolates; beyond them it is flat.
        let mid = c.apply(0.75);
        assert!(mid > c.apply(0.6) && mid < c.apply(0.9));
        assert!((c.apply(0.99) - c.apply(0.9)).abs() < 1e-9);
        assert!((c.apply(0.1) - c.apply(0.6)).abs() < 1e-9);
    }

    #[test]
    fn an_underconfident_model_is_lifted() {
        let samples = [at(0.7, 80, 78), at(0.3, 40, 4)].concat();
        let c = Calibrator::fit(&samples).unwrap();
        assert!(c.apply(0.7) > 0.95);
        assert!(c.apply(0.3) < 0.15);
    }

    #[test]
    fn violations_are_pooled_so_the_curve_never_decreases() {
        // 0.8 did worse than 0.7: the two are pooled into one level, 45 of
        // 60 accepted, centered at 0.75.
        let samples = [at(0.7, 30, 27), at(0.8, 30, 18), at(0.95, 30, 29)].concat();
        let c = Calibrator::fit(&samples).unwrap();
        assert_eq!(c.x.len(), 2);
        assert!((c.x[0] - 0.75).abs() < 1e-9);
        assert!((c.y[0] - 46.0 / 62.0).abs() < 1e-9);
        assert_eq!(c.apply(0.7), c.y[0]);
        let mut last = 0.0;
        for i in 0..=100 {
            let p = c.apply(i as f64 / 100.0);
            assert!(p >= last - 1e-12, "decreased at {i}");
            last = p;
        }
    }

    #[test]
    fn a_perfect_record_is_smoothed_below_certainty() {
        let c = Calibrator::fit(&at(0.8, 60, 60)).unwrap();
        assert!((c.apply(0.8) - 61.0 / 62.0).abs() < 1e-9);
        assert!(c.apply(0.8) < 1.0);
    }

    #[test]
    fn calibrators_round_trip_through_json() {
        let c = Calibrator::fit(&[at(0.9, 40, 30), at(0.5, 40, 10)].concat()).unwrap();
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Calibrator>(&json).unwrap(), c);
        assert_eq!(c.samples, 80);
    }

    #[test]
    fn reliability_bins_compare_promise_with_outcome() {
        let samples = [at(0.95, 10, 9), at(0.62, 4, 1), at(1.0, 2, 2)].concat();
        let bins = reliability(&samples, 10);
        assert_eq!(bins.len(), 2);
        assert_eq!(bins[0].count, 4);
        assert!((bins[0].actual - 0.25).abs() < 1e-9);
        assert_eq!(bins[1].count, 12);
        assert!((bins[1].actual - 11.0 / 12.0).abs() < 1e-9);
        let error = expected_error(&bins).unwrap();
        let expected = (4.0 * (0.62f64 - 0.25).abs()
            + 12.0 * ((0.95 * 10.0 + 2.0) / 12.0 - 11.0 / 12.0f64).abs())
            / 16.0;
        assert!((error - expected).abs() < 1e-9);
        assert_eq!(expected_error(&[]), None);
    }
}
