/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use once_cell::sync::Lazy;
use std::{mem, sync::RwLock};

use crate::provider::{DefaultTelemetryProvider, GlobalTelemetryProvider};

// Statically store the global provider
static GLOBAL_TELEMETRY_PROVIDER: Lazy<RwLock<GlobalTelemetryProvider>> =
    Lazy::new(|| RwLock::new(GlobalTelemetryProvider::new(DefaultTelemetryProvider::new())));

// Update the global provider
pub fn set_global_telemetry_provider(
    new_provider: GlobalTelemetryProvider,
) -> GlobalTelemetryProvider {
    // TODO(smithyObservability): would probably be nicer to return a Result here, but the Guard held by the error from
    // .try_write is not Send so struggled to build an ObservabilityError from it
    let mut old_provider = GLOBAL_TELEMETRY_PROVIDER.try_write().unwrap();
    mem::replace(&mut *old_provider, new_provider)
}
