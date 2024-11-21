/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utilities for interacting with the currently set `GlobalTelemetryProvider`

use once_cell::sync::Lazy;
use std::{
    mem,
    sync::{Arc, RwLock},
};

use crate::provider::{GlobalTelemetryProvider, TelemetryProvider};

// Statically store the global provider
static GLOBAL_TELEMETRY_PROVIDER: Lazy<RwLock<GlobalTelemetryProvider>> =
    Lazy::new(|| RwLock::new(GlobalTelemetryProvider::new(TelemetryProvider::default())));

/// Set the current global [TelemetryProvider].
///
/// This is meant to be run once at the beginning of an application. It will panic if two threads
/// attempt to call it at the same time.
pub fn set_telemetry_provider(new_provider: TelemetryProvider) {
    // TODO(smithyObservability): would probably be nicer to return a Result here, but the Guard held by the error from
    // .try_write is not Send so I struggled to build an ObservabilityError from it
    let mut old_provider = GLOBAL_TELEMETRY_PROVIDER
        .try_write()
        .expect("GLOBAL_TELEMETRY_PROVIDER RwLock Poisoned");

    let new_global_provider = GlobalTelemetryProvider::new(new_provider);

    let _ = mem::replace(&mut *old_provider, new_global_provider);
}

/// Get an [Arc] reference to the current global [TelemetryProvider]. [None] is returned if the [RwLock] containing
/// the global [TelemetryProvider] is poisoned or is currently locked by a writer.
pub fn get_telemetry_provider() -> Option<Arc<TelemetryProvider>> {
    // TODO(smithyObservability): would probably make more sense to return a Result rather than an Option here, but the Guard held by the error from
    // .try_read is not Send so I struggled to build an ObservabilityError from it
    if let Ok(tp) = GLOBAL_TELEMETRY_PROVIDER.try_read() {
        Some(tp.telemetry_provider().clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TelemetryProvider;
    use serial_test::serial;

    // Note: the tests in this module are run serially to prevent them from stepping on each other and poisoning the
    // RwLock holding the GlobalTelemetryProvider
    #[test]
    #[serial]
    fn can_set_global_telemetry_provider() {
        let my_provider = TelemetryProvider::default();

        // Set the new counter and get a reference to the old one
        set_telemetry_provider(my_provider);
    }

    #[test]
    #[serial]
    fn can_get_global_telemetry_provider() {
        let curr_provider = get_telemetry_provider().unwrap();

        // Use the global provider to create an instrument and record a value with it
        let curr_mp = curr_provider.meter_provider();
        let curr_meter = curr_mp.get_meter("TestMeter", None);
        let instrument = curr_meter.create_monotonic_counter("TestMonoCounter".into(), None, None);
        instrument.add(4, None, None);
    }
}
