/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Filter outliers using IQR method (1.5 × IQR fence).
/// Input must be sorted. Returns a new vec with outliers removed.
pub fn filter_outliers(sorted: &[f64]) -> Vec<f64> {
    let n = sorted.len();
    let q1 = sorted[n / 4];
    let q3 = sorted[3 * n / 4];
    let iqr = q3 - q1;
    let lo = q1 - 1.5 * iqr;
    let hi = q3 + 1.5 * iqr;
    sorted
        .iter()
        .copied()
        .filter(|&x| x >= lo && x <= hi)
        .collect()
}

/// Compute the median of a pre-sorted slice.
pub fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

/// Compute the p-th percentile (0.0–1.0) of a pre-sorted slice.
pub fn percentile<T: Copy + PartialOrd>(sorted: &[T], p: f64) -> T {
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx]
}

pub struct BasicStats {
    pub mean: f64,
    pub max: f64,
    pub std_dev: f64,
}

/// Compute mean, max, and population standard deviation for a set of samples.
pub fn basic_stats(samples: &[f64]) -> BasicStats {
    if samples.is_empty() {
        return BasicStats {
            mean: 0.0,
            max: 0.0,
            std_dev: 0.0,
        };
    }
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let std_dev =
        (samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    BasicStats { mean, max, std_dev }
}
