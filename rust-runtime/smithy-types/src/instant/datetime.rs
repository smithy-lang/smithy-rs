/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code is taken from https://github.com/pyfisch/httpdate and modified under an
// Apache 2.0 License. Modifications:
// - Trait implementations were removed and replaced with free
//   functions for conversion to and from strings. These have moved to instant/format.rs
// - Use of unsafe was removed and replaced with a panic
// - HTTPDate was renamed `StructuredDate` and `nanos` was added to support subsecond precision
// - Conversion to and from strings was updated to support fractional seconds

pub const NANOS_PER_SECOND: u32 = 1_000_000_000;

/// StructuredDate type
///
/// Used to convert between structured dates for
/// formatting & epoch seconds for representation
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StructuredDate {
    pub nanos: u32,
    /// 0...59
    pub sec: u8,
    /// 0...59
    pub min: u8,
    /// 0...23
    pub hour: u8,
    /// 1...31
    pub day: u8,
    /// 1...12
    pub mon: u8,
    /// 1970...9999
    pub year: u16,
    /// 1...7
    pub wday: u8,
}

impl StructuredDate {
    pub fn is_valid(&self) -> bool {
        self.sec < 60
            && self.min < 60
            && self.hour < 24
            && self.day > 0
            && self.day < 32
            && self.mon > 0
            && self.mon <= 12
            && self.year >= 1970
            && self.year <= 9999
            && self.nanos < NANOS_PER_SECOND
    }

    pub fn from_epoch_secs(secs_since_epoch: i64, subsecond_nanos: u32) -> StructuredDate {
        if secs_since_epoch >= 253402300800 {
            // year 9999
            panic!("date must be before year 9999");
        }

        if secs_since_epoch < 0 {
            // We should support pre-epoch date eventually
            todo!() // https://github.com/awslabs/smithy-rs/pull/86
        }

        /* 2000-03-01 (mod 400 year, immediately after feb29 */
        const LEAPOCH: i64 = 11017;
        const DAYS_PER_400Y: i64 = 365 * 400 + 97;
        const DAYS_PER_100Y: i64 = 365 * 100 + 24;
        const DAYS_PER_4Y: i64 = 365 * 4 + 1;

        let days = (secs_since_epoch / 86400) as i64 - LEAPOCH;
        let secs_of_day = secs_since_epoch % 86400;

        let mut qc_cycles = days / DAYS_PER_400Y;
        let mut remdays = days % DAYS_PER_400Y;

        if remdays < 0 {
            remdays += DAYS_PER_400Y;
            qc_cycles -= 1;
        }

        let mut c_cycles = remdays / DAYS_PER_100Y;
        if c_cycles == 4 {
            c_cycles -= 1;
        }
        remdays -= c_cycles * DAYS_PER_100Y;

        let mut q_cycles = remdays / DAYS_PER_4Y;
        if q_cycles == 25 {
            q_cycles -= 1;
        }
        remdays -= q_cycles * DAYS_PER_4Y;

        let mut remyears = remdays / 365;
        if remyears == 4 {
            remyears -= 1;
        }
        remdays -= remyears * 365;

        let mut year = 2000 + remyears + 4 * q_cycles + 100 * c_cycles + 400 * qc_cycles;

        let months = [31, 30, 31, 30, 31, 31, 30, 31, 30, 31, 31, 29];
        let mut mon = 0;
        for mon_len in months.iter() {
            mon += 1;
            if remdays < *mon_len {
                break;
            }
            remdays -= *mon_len;
        }
        let mday = remdays + 1;
        let mon = if mon + 2 > 12 {
            year += 1;
            mon - 10
        } else {
            mon + 2
        };

        let mut wday = (3 + days) % 7;
        if wday <= 0 {
            wday += 7
        };

        StructuredDate {
            nanos: subsecond_nanos,
            sec: (secs_of_day % 60) as u8,
            min: ((secs_of_day % 3600) / 60) as u8,
            hour: (secs_of_day / 3600) as u8,
            day: mday as u8,
            mon: mon as u8,
            year: year as u16,
            wday: wday as u8,
        }
    }

    pub fn to_epoch_secs(&self) -> (i64, u32) {
        let v = self;
        let leap_years =
            ((v.year - 1) - 1968) / 4 - ((v.year - 1) - 1900) / 100 + ((v.year - 1) - 1600) / 400;
        let mut ydays = match v.mon {
            1 => 0,
            2 => 31,
            3 => 59,
            4 => 90,
            5 => 120,
            6 => 151,
            7 => 181,
            8 => 212,
            9 => 243,
            10 => 273,
            11 => 304,
            12 => 334,
            _ => unreachable!(),
        } + v.day as u64
            - 1;
        if is_leap_year(v.year) && v.mon > 2 {
            ydays += 1;
        }
        let days = (v.year as i64 - 1970) * 365 + leap_years as i64 + ydays as i64;
        (
            (v.sec as u64 + v.min as u64 * 60 + v.hour as u64 * 3600) as i64 + days * 86400,
            v.nanos,
        )
    }
}

fn is_leap_year(y: u16) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
