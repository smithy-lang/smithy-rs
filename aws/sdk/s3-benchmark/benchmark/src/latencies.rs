/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt;
use std::time;

const ONE_GIGABYTE: u64 = 1000 * 1000 * 1000;

#[derive(Debug)]
pub struct Latencies {
    object_size_bytes: u64,
    raw_values: Vec<f64>,
}

impl Latencies {
    pub fn new(object_size_bytes: u64) -> Self {
        Self {
            object_size_bytes,
            raw_values: Vec::new(),
        }
    }

    pub fn push(&mut self, value: time::Duration) {
        self.raw_values.push(value.as_secs_f64());
    }

    /// Calculates the standard deviation squared of the given values.
    fn variance(values: &[f64], average: f64) -> f64 {
        values
            .iter()
            .map(|value| (value - average).powi(2))
            .sum::<f64>()
            / values.len() as f64
    }
}

impl fmt::Display for Latencies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let object_size_gigabits = self.object_size_bytes as f64 / ONE_GIGABYTE as f64 * 8f64;

        let average_latency = self.raw_values.iter().sum::<f64>() / self.raw_values.len() as f64;
        let lowest_latency = self
            .raw_values
            .iter()
            .fold(std::f64::INFINITY, |acc, &x| acc.min(x));
        let variance = Self::variance(&self.raw_values, average_latency);
        writeln!(f, "Latency values (s): {:?}", self.raw_values)?;
        writeln!(f, "Average latency (s): {average_latency}")?;
        writeln!(f, "Latency variance (s): {variance}")?;
        writeln!(f, "Object size (Gigabits): {object_size_gigabits}")?;
        writeln!(
            f,
            "Average throughput (Gbps): {}",
            object_size_gigabits / average_latency
        )?;
        writeln!(
            f,
            "Highest average throughput (Gbps): {}",
            object_size_gigabits / lowest_latency
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::latencies::{Latencies, ONE_GIGABYTE};

    #[test]
    fn test_display() {
        let latencies = Latencies {
            object_size_bytes: 30 * ONE_GIGABYTE,
            raw_values: vec![
                33.261f64, 41.114, 33.014, 32.97, 34.138, 33.972, 33.001, 34.12,
            ],
        };

        let expected = "\
            Latency values (s): [33.261, 41.114, 33.014, 32.97, 34.138, 33.972, 33.001, 34.12]\n\
            Average latency (s): 34.448750000000004\n\
            Latency variance (s): 6.576178687499994\n\
            Object size (Gigabits): 240\n\
            Average throughput (Gbps): 6.966871076599295\n\
            Highest average throughput (Gbps): 7.279344858962694\n";
        let actual = latencies.to_string();
        assert_eq!(expected, actual);
    }
}
