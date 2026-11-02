/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Events that a generated client's response checksum validation reports to observers.
//!
//! Observers are registered in the config bag under [`SharedResponseChecksumObserver`]. Because
//! these types are in a 1.x crate, a client and an observer built against different releases of
//! the crates around them still find each other.

use aws_smithy_types::config_bag::{Storable, StoreAppend};
use std::fmt;
use std::sync::Arc;

/// Receives the response checksum validation events of one operation.
///
/// A generated client's response checksum validation calls these methods. Each attempt that
/// reaches the checksum decision produces either [`not_validated`](Self::not_validated), or
/// [`validating`](Self::validating) followed by one ending per wrapped body:
/// [`matched`](Self::matched), [`mismatched`](Self::mismatched), [`errored`](Self::errored) or
/// [`dropped`](Self::dropped). Methods are called on the task that drives the attempt or reads the
/// body, and must not block.
///
/// Every method has a default that does nothing, so that events can be added.
pub trait ObserveResponseChecksum: Send + Sync + fmt::Debug {
    /// A validator was attached to the response body.
    ///
    /// `algorithm` is the lowercase name of the checksum algorithm: `"crc32"`, `"crc32c"`,
    /// `"crc64nvme"`, `"sha1"` or `"sha256"` today. Later releases may add names.
    fn validating(&self, algorithm: &'static str) {
        let _ = algorithm;
    }

    /// The response will not be validated.
    fn not_validated(&self, reason: ChecksumSkip) {
        let _ = reason;
    }

    /// The body's data ended and its checksum matched.
    fn matched(&self) {}

    /// The body's data ended and its checksum did not match.
    ///
    /// Both values are base64. This is called before the body yields its mismatch error.
    fn mismatched(&self, expected: String, computed: String) {
        let _ = (expected, computed);
    }

    /// The body yielded an error before its data ended.
    fn errored(&self) {}

    /// The body was dropped before its data ended.
    fn dropped(&self) {}
}

/// Why a response's checksum will not be validated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChecksumSkip {
    /// Validation is disabled by the request's checksum mode or by the client's
    /// `response_checksum_validation` setting.
    Disabled,
    /// The response has no checksum header for an algorithm the operation lists.
    NoChecksum,
    /// The checksum header holds a part-level value, which ends in `-N`.
    PartLevelChecksum,
    /// The checksum header is present but is not valid base64.
    InvalidChecksumHeader,
    /// The only checksum header is for an algorithm the client does not compute.
    UnsupportedAlgorithm,
}

/// A shared [`ObserveResponseChecksum`], and the config-bag key that observers are registered
/// under.
///
/// The key appends, so every registered observer receives every event.
#[derive(Clone, Debug)]
pub struct SharedResponseChecksumObserver(Arc<dyn ObserveResponseChecksum>);

impl SharedResponseChecksumObserver {
    /// Creates a new [`SharedResponseChecksumObserver`] from an observer.
    pub fn new(observer: impl ObserveResponseChecksum + 'static) -> Self {
        Self(Arc::new(observer))
    }
}

impl ObserveResponseChecksum for SharedResponseChecksumObserver {
    fn validating(&self, algorithm: &'static str) {
        self.0.validating(algorithm)
    }

    fn not_validated(&self, reason: ChecksumSkip) {
        self.0.not_validated(reason)
    }

    fn matched(&self) {
        self.0.matched()
    }

    fn mismatched(&self, expected: String, computed: String) {
        self.0.mismatched(expected, computed)
    }

    fn errored(&self) {
        self.0.errored()
    }

    fn dropped(&self) {
        self.0.dropped()
    }
}

impl Storable for SharedResponseChecksumObserver {
    type Storer = StoreAppend<Self>;
}

#[cfg(test)]
mod tests {
    use super::{ChecksumSkip, ObserveResponseChecksum, SharedResponseChecksumObserver};
    use aws_smithy_types::config_bag::{ConfigBag, Layer};
    use std::sync::{Arc, Mutex};

    /// Records the events it receives.
    #[derive(Clone, Debug, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl Recorder {
        fn record(&self, event: impl Into<String>) {
            self.0.lock().unwrap().push(event.into());
        }

        fn events(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl ObserveResponseChecksum for Recorder {
        fn validating(&self, algorithm: &'static str) {
            self.record(format!("validating {algorithm}"));
        }

        fn not_validated(&self, reason: ChecksumSkip) {
            self.record(format!("not_validated {reason:?}"));
        }

        fn matched(&self) {
            self.record("matched");
        }

        fn mismatched(&self, expected: String, computed: String) {
            self.record(format!("mismatched {expected} {computed}"));
        }

        fn errored(&self) {
            self.record("errored");
        }

        fn dropped(&self) {
            self.record("dropped");
        }
    }

    #[test]
    fn every_appended_observer_receives_every_event() {
        let (first, second) = (Recorder::default(), Recorder::default());
        let mut layer = Layer::new("test");
        layer.store_append(SharedResponseChecksumObserver::new(first.clone()));
        layer.store_append(SharedResponseChecksumObserver::new(second.clone()));
        let cfg = ConfigBag::of_layers(vec![layer]);

        for observer in cfg.load::<SharedResponseChecksumObserver>() {
            observer.validating("crc32");
            observer.not_validated(ChecksumSkip::NoChecksum);
            observer.matched();
            observer.mismatched("expected".into(), "computed".into());
            observer.errored();
            observer.dropped();
        }

        let expected = [
            "validating crc32",
            "not_validated NoChecksum",
            "matched",
            "mismatched expected computed",
            "errored",
            "dropped",
        ];
        assert_eq!(first.events(), expected);
        assert_eq!(second.events(), expected);
    }
}
