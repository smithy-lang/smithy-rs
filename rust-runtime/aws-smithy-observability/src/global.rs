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

/// Set the current global [TelemetryProvider]. If [None] is supplied then a noop provider is set.
/// The previous [TelemetryProvider] is returned in an [Arc] so appropriate cleanup can be done if necessary.
pub fn set_global_telemetry_provider(
    new_provider: Option<TelemetryProvider>,
) -> Arc<TelemetryProvider> {
    // TODO(smithyObservability): would probably be nicer to return a Result here, but the Guard held by the error from
    // .try_write is not Send so I struggled to build an ObservabilityError from it
    let mut old_provider = GLOBAL_TELEMETRY_PROVIDER
        .try_write()
        .expect("GLOBAL_TELEMETRY_PROVIDER RwLock Poisoned");

    let new_global_provider = if let Some(tp) = new_provider {
        GlobalTelemetryProvider::new(tp)
    } else {
        GlobalTelemetryProvider::new(TelemetryProvider::default())
    };

    mem::replace(&mut *old_provider, new_global_provider).telemetry_provider
}

/// Get an [Arc] reference to the current global [TelemetryProvider].
pub fn get_global_telemetry_provider() -> Arc<TelemetryProvider> {
    // TODO(smithyObservability): would probably be nicer to return a Result here, but the Guard held by the error from
    // .try_read is not Send so I struggled to build an ObservabilityError from it
    GLOBAL_TELEMETRY_PROVIDER
        .try_read()
        .expect("GLOBAL_TELEMETRY_PROVIDER RwLock Poisoned")
        .telemetry_provider()
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TelemetryProvider;

    #[test]
    fn can_set_global_telemetry_provider() {
        let my_provider = TelemetryProvider::default();

        // Set the new counter and get a reference to the old one
        let old_provider = set_global_telemetry_provider(Some(my_provider));

        // Call shutdown on the old meter provider
        let _old_meter = old_provider.meter_provider().shutdown().unwrap();
    }

    #[test]
    fn can_get_global_telemetry_provider() {
        let curr_provider = get_global_telemetry_provider();

        // Use the global provider to create an instrument and record a value with it
        let curr_mp = curr_provider.meter_provider();
        let curr_meter = curr_mp.get_meter("TestMeter", None);
        let instrument = curr_meter.create_monotonic_counter("TestMonoCounter".into(), None, None);
        instrument.add(4, None, None);
    }
}
