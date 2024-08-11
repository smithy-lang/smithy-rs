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

/// Set the current global [TelemetryProvider], the previous [TelemetryProvider] is returned
/// in an [Arc] so appropriate cleanup can be done if necessary.
pub fn set_global_telemetry_provider(new_provider: TelemetryProvider) -> Arc<TelemetryProvider> {
    // TODO(smithyObservability): would probably be nicer to return a Result here, but the Guard held by the error from
    // .try_write is not Send so I struggled to build an ObservabilityError from it
    let mut old_provider = GLOBAL_TELEMETRY_PROVIDER
        .try_write()
        .expect("GLOBAL_TELEMETRY_PROVIDER RwLock Poisoned");

    let new_global_provider = GlobalTelemetryProvider::new(new_provider);

    mem::replace(&mut *old_provider, new_global_provider).telemetry_provider
}

/// Get an [Arc] reference to the current global [TelemetryProvider].
pub fn global_telemetry_provider() -> Arc<TelemetryProvider> {
    let foo = GLOBAL_TELEMETRY_PROVIDER
        .try_read()
        .expect("GLOBAL_TELEMETRY_PROVIDER RwLock Poisoned");

    foo.telemetry_provider().clone()
}
