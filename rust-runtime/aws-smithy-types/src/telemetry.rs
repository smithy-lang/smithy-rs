/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Types for carrying values selected from an operation's input through to telemetry.
//!
//! Telemetry runs at the generic layer, where the operation input is type-erased and, during
//! serialization, consumed before any result-bearing hook runs. After that point a value such as
//! the target resource identifier survives only inside the serialized request. `CapturedTelemetryAttributes`
//! is the bridge: generated code selects an input member once, before the input is consumed, and
//! writes it here into the `ConfigBag`. Any downstream interceptor — and the built-in metrics
//! implementation — can then read it via `cfg.load`.
//!
//! This type lives in `aws-smithy-types` (a stable crate) deliberately: it carries no dependency on
//! `aws-smithy-observability` and can therefore appear in stable, generated configuration without
//! leaking a 0.x type.
//!
//! It is off by default. When no input member is selected, nothing is captured and this value is
//! absent from the `ConfigBag`.

use crate::config_bag::{Storable, StoreReplace};
use std::collections::HashMap;
use std::sync::Arc;

/// A set of string-keyed values selected from an operation's input, carried through the `ConfigBag`
/// for telemetry.
///
/// Keys and values are `Arc<str>` so that cloning the bag — as happens when it propagates through
/// config-bag layers — stays cheap regardless of value length.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapturedTelemetryAttributes {
    values: HashMap<Arc<str>, Arc<str>>,
}

impl CapturedTelemetryAttributes {
    /// Creates an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a captured value under `name`, replacing any existing value for that name.
    pub fn insert(&mut self, name: impl Into<Arc<str>>, value: impl Into<Arc<str>>) {
        self.values.insert(name.into(), value.into());
    }

    /// Returns the captured value for `name`, if one was captured.
    ///
    /// This is the read path for a downstream interceptor that wants a captured value directly,
    /// e.g. `cfg.load::<CapturedTelemetryAttributes>().and_then(|a| a.get("Bucket"))`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(|v| v.as_ref())
    }

    /// Iterates over the captured `(name, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_ref(), v.as_ref()))
    }
}

impl Storable for CapturedTelemetryAttributes {
    type Storer = StoreReplace<Self>;
}

/// The set of operation-input member names a customer has opted in to record on telemetry.
///
/// This is the customer's selection, set once on the service config. The generated per-operation
/// interceptor reads it, captures the matching input members into `CapturedTelemetryAttributes`,
/// and the built-in metrics carry them. Absent unless the customer opts in, so capture is a no-op
/// by default.
///
/// Names are the Smithy member names (e.g. `"Bucket"`), matched by generated code against the
/// operation's input members.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestedTelemetryAttributes {
    names: Vec<Arc<str>>,
}

impl RequestedTelemetryAttributes {
    /// Creates a selection from an iterator of member names.
    pub fn new(names: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns `true` if `name` was requested.
    pub fn contains(&self, name: &str) -> bool {
        self.names.iter().any(|n| n.as_ref() == name)
    }

    /// Returns `true` if nothing was requested.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

impl Storable for RequestedTelemetryAttributes {
    type Storer = StoreReplace<Self>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut attrs = CapturedTelemetryAttributes::new();
        assert_eq!(attrs.iter().count(), 0);

        attrs.insert("bucket", "example-bucket");
        assert_eq!(attrs.get("bucket"), Some("example-bucket"));
        assert_eq!(attrs.get("missing"), None);
        assert_eq!(attrs.iter().count(), 1);
    }

    #[test]
    fn insert_replaces_existing() {
        let mut attrs = CapturedTelemetryAttributes::new();
        attrs.insert("bucket", "first");
        attrs.insert("bucket", "second");
        assert_eq!(attrs.get("bucket"), Some("second"));
        assert_eq!(attrs.iter().count(), 1);
    }

    #[test]
    fn iter_yields_all_pairs() {
        let mut attrs = CapturedTelemetryAttributes::new();
        attrs.insert("bucket", "b");
        attrs.insert("table", "t");
        let mut pairs: Vec<_> = attrs.iter().collect();
        pairs.sort();
        assert_eq!(pairs, vec![("bucket", "b"), ("table", "t")]);
    }

    #[test]
    fn requested_selection() {
        let requested = RequestedTelemetryAttributes::new(["Bucket", "Key"]);
        assert!(requested.contains("Bucket"));
        assert!(requested.contains("Key"));
        assert!(!requested.contains("VersionId"));
        assert!(!requested.is_empty());

        let empty = RequestedTelemetryAttributes::default();
        assert!(empty.is_empty());
        assert!(!empty.contains("Bucket"));
    }
}
