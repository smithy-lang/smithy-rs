/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utilities for interacting with the currently set `GlobalTelemetryProvider`

use once_cell::sync::Lazy;
use std::{
    any::Any,
    mem,
    sync::{Arc, RwLock},
};

use crate::{
    error::{ErrorKind, GlobalTelemetryProviderError},
    meter::ProvideMeter,
    provider::{GlobalTelemetryProvider, TelemetryProvider},
    ObservabilityError,
};

// Statically store the global provider
static GLOBAL_TELEMETRY_PROVIDER: Lazy<RwLock<Box<dyn Any + Send + Sync>>> = Lazy::new(|| {
    RwLock::new(Box::new(GlobalTelemetryProvider::new(
        TelemetryProvider::default(),
    )))
});

/// Set the current global [TelemetryProvider].
///
/// This is meant to be run once at the beginning of an application. Will return an [Err] if the
/// [RwLock] holding the global [TelemetryProvider] is locked or poisoned.
pub fn set_telemetry_provider<PM: ProvideMeter + 'static>(
    new_provider: TelemetryProvider<PM>,
) -> Result<(), ObservabilityError> {
    if let Ok(mut old_provider) = GLOBAL_TELEMETRY_PROVIDER.try_write() {
        let new_global_provider = Box::new(GlobalTelemetryProvider::new(new_provider));

        let _ = mem::replace(&mut *old_provider, new_global_provider);

        Ok(())
    } else {
        Err(ObservabilityError::new(
            ErrorKind::SettingGlobalProvider,
            GlobalTelemetryProviderError::new(
                "Failed to set TelemetryProvider, likely because the RwLock holding it is locked.",
            ),
        ))
    }
}

/// Get an [Arc] reference to the current global [TelemetryProvider]. Will return an [Err] if the
/// [RwLock] holding the global [TelemetryProvider] is locked or poisoned.
pub fn get_telemetry_provider<PM: ProvideMeter + 'static>(
) -> Result<Arc<TelemetryProvider<PM>>, ObservabilityError> {
    if let Ok(tp) = GLOBAL_TELEMETRY_PROVIDER.try_read() {
        if let Some(typed_tp) = tp.downcast_ref::<GlobalTelemetryProvider<PM>>() {
            Ok(typed_tp.telemetry_provider().clone())
        } else {
            Err(ObservabilityError::new(
                ErrorKind::GettingGlobalProvider,
                GlobalTelemetryProviderError::new(
                    "Failed to downcast TelemetryProvider to the provided type.",
                ),
            ))
        }
    } else {
        Err(ObservabilityError::new(
            ErrorKind::GettingGlobalProvider,
            GlobalTelemetryProviderError::new("Failed to get the TelemetryProvider, likely because the RwLock containing it is locked."),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        meter::{Meter, MonotonicCounter},
        noop::NoopMeterProvider,
        provider::TelemetryProvider,
    };
    use serial_test::serial;

    // Note: the tests in this module are run serially to prevent them from stepping on each other and poisoning the
    // RwLock holding the GlobalTelemetryProvider
    #[test]
    #[serial]
    fn can_set_global_telemetry_provider() {
        let my_provider = TelemetryProvider::default();

        // Set the new counter and get a reference to the old one
        set_telemetry_provider(my_provider).unwrap();
    }

    #[test]
    #[serial]
    fn can_get_global_telemetry_provider() {
        let curr_provider = get_telemetry_provider::<NoopMeterProvider>().unwrap();

        // Use the global provider to create an instrument and record a value with it
        let curr_mp = curr_provider.meter_provider();
        let curr_meter = curr_mp.get_meter("TestMeter", None);
        let instrument = curr_meter.create_monotonic_counter("TestMonoCounter".into(), None, None);
        instrument.add(4, None, None);
    }
}
