/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection placement declarations.
//!
//! A [`Partition`] combines stable identity, driver placement, and an optional
//! network-interface binding. It declares where connections live; sharing a
//! connection never moves that connection's I/O or driver.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Declares where a client's connections are established and driven.
#[derive(Clone)]
pub struct Partition {
    id: PartitionId,
    spawner: Arc<dyn DriverSpawner>,
    interface: Option<Arc<str>>,
}

impl Partition {
    /// Creates a partition with a stable identity and driver spawner.
    pub fn new<S>(id: PartitionId, spawner: S) -> Self
    where
        S: DriverSpawner,
    {
        Self {
            id,
            spawner: Arc::new(spawner),
            interface: None,
        }
    }

    /// Binds connections established by this partition to an interface.
    ///
    /// The binding is applied before connect. On Linux, using this setting
    /// may require `CAP_NET_RAW` or root privileges.
    #[cfg(any(
        target_os = "android",
        target_os = "fuchsia",
        target_os = "illumos",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "solaris",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos",
    ))]
    pub fn interface(mut self, interface: impl Into<String>) -> Self {
        self.interface = Some(Arc::from(interface.into()));
        self
    }

    /// Returns this partition's declared identity.
    pub(super) fn id(&self) -> PartitionId {
        self.id
    }

    /// Decomposes this declaration for immutable registry storage.
    pub(super) fn into_parts(self) -> (PartitionId, Arc<dyn DriverSpawner>, Option<Arc<str>>) {
        (self.id, self.spawner, self.interface)
    }
}

impl fmt::Debug for Partition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Partition")
            .field("id", &self.id)
            .field("spawner", &self.spawner)
            .field("interface", &self.interface)
            .finish()
    }
}

/// Identifies one declared connection partition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PartitionId(usize);

impl PartitionId {
    /// Reserved identity for the implicit default partition.
    pub const ANONYMOUS: Self = Self(usize::MAX);

    /// Creates a partition identity from a caller-owned index.
    ///
    /// `usize::MAX` is reserved for [`Self::ANONYMOUS`] and is rejected when
    /// used in an explicit partition declaration.
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }

    /// Returns whether this is the reserved anonymous partition identity.
    pub const fn is_anonymous(self) -> bool {
        self.0 == Self::ANONYMOUS.0
    }
}

/// Spawns protocol drivers on a partition's owning runtime.
pub trait DriverSpawner: fmt::Debug + Send + Sync + 'static {
    /// Spawns a protocol driver future.
    fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>);
}

/// A driver spawner backed by a captured Tokio runtime handle.
#[cfg(feature = "rt-tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "rt-tokio")))]
#[derive(Clone, Debug)]
pub struct TokioDriverSpawner {
    handle: tokio::runtime::Handle,
}

#[cfg(feature = "rt-tokio")]
impl TokioDriverSpawner {
    /// Captures the current Tokio runtime.
    ///
    /// # Panics
    ///
    /// Panics when called outside a Tokio runtime context.
    pub fn current() -> Self {
        Self::from_handle(tokio::runtime::Handle::current())
    }

    /// Uses a specific Tokio runtime handle.
    pub fn from_handle(handle: tokio::runtime::Handle) -> Self {
        Self { handle }
    }
}

#[cfg(feature = "rt-tokio")]
impl DriverSpawner for TokioDriverSpawner {
    fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
        debug_assert_eq!(
            tokio::runtime::Handle::try_current()
                .ok()
                .map(|current| current.id()),
            Some(self.handle.id()),
            "driver spawned outside the partition's declared runtime"
        );
        drop(self.handle.spawn(driver));
    }
}

/// Controls which partitions may dispatch through each other's connections.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConnectionReuseScope {
    /// A connection serves only requests from its owning partition.
    Partition,
    /// Partitions with the same interface binding may share connections.
    #[default]
    NetworkInterface,
    /// Every partition in the pool may share connections.
    Pool,
}

/// Identifies the exact set of partitions eligible to reuse a connection.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum EligibilityGroup {
    /// Only the partition with this identity is eligible.
    Partition(PartitionId),
    /// Partitions with this exact interface binding are eligible.
    NetworkInterface(Option<Arc<str>>),
    /// Every partition in the pool is eligible.
    Pool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "rt-tokio")]
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Debug)]
    struct TestSpawner;

    impl DriverSpawner for TestSpawner {
        fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
            drop(driver);
        }
    }

    #[test]
    fn anonymous_identity_is_reserved() {
        assert!(PartitionId::ANONYMOUS.is_anonymous());
        assert!(PartitionId::from_index(usize::MAX).is_anonymous());
        assert!(!PartitionId::from_index(0).is_anonymous());
    }

    #[test]
    fn partition_retains_placement() {
        let partition = Partition::new(PartitionId::from_index(7), TestSpawner);
        let (id, spawner, interface) = partition.into_parts();
        assert_eq!(PartitionId::from_index(7), id);
        assert_eq!(None, interface);

        let driver = Box::pin(async {});
        spawner.spawn(driver);
    }

    #[cfg(feature = "rt-tokio")]
    #[test]
    fn tokio_spawner_uses_captured_runtime() {
        let owner = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let owner_id = owner.handle().id();
        let ran = Arc::new(AtomicBool::new(false));
        let task_ran = ran.clone();
        let spawner = TokioDriverSpawner::from_handle(owner.handle().clone());

        owner.block_on(async {
            spawner.spawn(Box::pin(async move {
                assert_eq!(owner_id, tokio::runtime::Handle::current().id());
                task_ran.store(true, Ordering::SeqCst);
            }));
            for _ in 0..10 {
                if ran.load(Ordering::SeqCst) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        });
        assert!(ran.load(Ordering::SeqCst));
    }

    #[cfg(all(feature = "rt-tokio", debug_assertions))]
    #[test]
    #[should_panic(expected = "driver spawned outside the partition's declared runtime")]
    fn tokio_spawner_diagnoses_foreign_runtime_use() {
        let owner = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let foreign = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let spawner = TokioDriverSpawner::from_handle(owner.handle().clone());

        foreign.block_on(async {
            spawner.spawn(Box::pin(async {}));
        });
    }
}
