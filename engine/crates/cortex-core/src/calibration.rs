//! Trajectory-level confidence calibration.
//!
//! A small calibrator usable as a jury/GDS input. Maps raw confidence scores
//! to calibrated (observed) confidences via binned tracking.

use serde::{Deserialize, Serialize};

/// Confidence calibrator that tracks predicted vs observed accuracy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceCalibrator {
    /// List of (predicted_confidence, correct) observations.
    bins: Vec<(f64, bool)>,
}

impl ConfidenceCalibrator {
    /// Create a new calibrator.
    pub fn new() -> Self {
        Self { bins: Vec::new() }
    }

    /// Record an observation of predicted confidence and actual correctness.
    pub fn observe(&mut self, predicted: f64, correct: bool) {
        self.bins.push((predicted.clamp(0.0, 1.0), correct));
    }

    /// Return the calibrated confidence for a raw confidence score.
    ///
    /// Uses a simple binned approach: groups observations by confidence bucket
    /// (10 buckets: [0-0.1), [0.1-0.2), ... [0.9-1.0]) and returns the empirical
    /// success rate in the matching bucket. Falls back to raw confidence if no
    /// observations in that bucket.
    pub fn calibrated(&self, raw_confidence: f64) -> f64 {
        let clamped = raw_confidence.clamp(0.0, 1.0);

        // Determine which bucket this confidence falls into (10 buckets).
        let bucket_size = 0.1;
        let bucket_idx = ((clamped / bucket_size).floor() as usize).min(9);
        let bucket_min = bucket_idx as f64 * bucket_size;
        let bucket_max = (bucket_idx + 1) as f64 * bucket_size;

        // Find observations in this bucket.
        let in_bucket: Vec<_> = self
            .bins
            .iter()
            .filter(|(pred, _)| *pred >= bucket_min && *pred < bucket_max)
            .collect();

        if in_bucket.is_empty() {
            // No calibration data; return raw.
            return clamped;
        }

        // Empirical success rate in this bucket.
        let successes = in_bucket.iter().filter(|(_, correct)| *correct).count();
        successes as f64 / in_bucket.len() as f64
    }

    /// Compute the Brier score (mean squared error between predicted and observed).
    pub fn brier_score(&self) -> f64 {
        if self.bins.is_empty() {
            return 0.0;
        }

        let sum: f64 = self
            .bins
            .iter()
            .map(|(pred, correct)| {
                let obs = if *correct { 1.0 } else { 0.0 };
                (pred - obs).powi(2)
            })
            .sum();

        sum / self.bins.len() as f64
    }

    /// Get the number of observations recorded.
    pub fn len(&self) -> usize {
        self.bins.len()
    }

    /// Check if the calibrator has no observations.
    pub fn is_empty(&self) -> bool {
        self.bins.is_empty()
    }
}

impl Default for ConfidenceCalibrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrator_observe_records_data() {
        let mut cal = ConfidenceCalibrator::new();
        assert_eq!(cal.len(), 0);
        cal.observe(0.9, true);
        assert_eq!(cal.len(), 1);
    }

    #[test]
    fn calibrator_empty() {
        let cal = ConfidenceCalibrator::new();
        assert!(cal.is_empty());
    }

    #[test]
    fn calibrator_calibrated_no_data_returns_raw() {
        let cal = ConfidenceCalibrator::new();
        let calib = cal.calibrated(0.75);
        assert_eq!(calib, 0.75);
    }

    #[test]
    fn calibrator_calibrated_single_correct_observation() {
        let mut cal = ConfidenceCalibrator::new();
        cal.observe(0.8, true);
        let calib = cal.calibrated(0.8);
        assert_eq!(calib, 1.0); // 100% success in that bucket
    }

    #[test]
    fn calibrator_calibrated_mixed_observations() {
        let mut cal = ConfidenceCalibrator::new();
        // Two correct, one incorrect in the 0.8-0.9 range.
        cal.observe(0.85, true);
        cal.observe(0.85, true);
        cal.observe(0.85, false);
        let calib = cal.calibrated(0.85);
        assert!((calib - 2.0 / 3.0).abs() < 0.01); // ~0.667
    }

    #[test]
    fn calibrator_calibrated_buckets_differently() {
        let mut cal = ConfidenceCalibrator::new();
        // High confidence bucket all correct.
        cal.observe(0.95, true);
        cal.observe(0.95, true);
        // Low confidence bucket all incorrect.
        cal.observe(0.15, false);
        cal.observe(0.15, false);

        let high = cal.calibrated(0.95);
        let low = cal.calibrated(0.15);

        assert_eq!(high, 1.0);
        assert_eq!(low, 0.0);
    }

    #[test]
    fn calibrator_brier_score_perfect() {
        let mut cal = ConfidenceCalibrator::new();
        cal.observe(1.0, true);
        cal.observe(0.0, false);
        let bs = cal.brier_score();
        assert!((bs - 0.0).abs() < 0.01);
    }

    #[test]
    fn calibrator_brier_score_worst() {
        let mut cal = ConfidenceCalibrator::new();
        cal.observe(1.0, false);
        cal.observe(0.0, true);
        let bs = cal.brier_score();
        assert!((bs - 1.0).abs() < 0.01);
    }

    #[test]
    fn calibrator_brier_score_mixed() {
        let mut cal = ConfidenceCalibrator::new();
        cal.observe(0.5, true); // error = (0.5 - 1.0)^2 = 0.25
        cal.observe(0.5, false); // error = (0.5 - 0.0)^2 = 0.25
        let bs = cal.brier_score();
        assert!((bs - 0.25).abs() < 0.01);
    }

    #[test]
    fn calibrator_clamps_inputs() {
        let mut cal = ConfidenceCalibrator::new();
        cal.observe(1.5, true); // Will be clamped to 1.0
        cal.observe(-0.5, false); // Will be clamped to 0.0

        let calib_high = cal.calibrated(1.5);
        let calib_low = cal.calibrated(-0.5);

        assert!((0.0..=1.0).contains(&calib_high));
        assert!((0.0..=1.0).contains(&calib_low));
    }

    #[test]
    fn calibrator_multiple_observations_per_bucket() {
        let mut cal = ConfidenceCalibrator::new();
        // Fill 0.5-0.6 bucket with 80% correctness.
        for _ in 0..4 {
            cal.observe(0.55, true);
        }
        for _ in 0..1 {
            cal.observe(0.55, false);
        }
        let calib = cal.calibrated(0.55);
        assert!((calib - 0.8).abs() < 0.01);
    }

    #[test]
    fn calibrator_far_buckets_separate() {
        let mut cal = ConfidenceCalibrator::new();
        cal.observe(0.15, true);
        cal.observe(0.15, true);
        cal.observe(0.85, false);
        cal.observe(0.85, false);

        let low = cal.calibrated(0.15);
        let high = cal.calibrated(0.85);

        assert_eq!(low, 1.0);
        assert_eq!(high, 0.0);
    }

    #[test]
    fn calibrator_boundary_values() {
        let mut cal = ConfidenceCalibrator::new();
        cal.observe(0.0, false);
        cal.observe(1.0, true);

        let calib_zero = cal.calibrated(0.0);
        let calib_one = cal.calibrated(1.0);

        assert_eq!(calib_zero, 0.0);
        assert_eq!(calib_one, 1.0);
    }
}
