/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Pool topology and stable origin-cell publication.
//!
//! [`PartitionRegistry`] retains the fixed partition set and one shared
//! [`OriginAdmission`] for each bounded origin. Each [`PartitionState`] owns
//! runtime placement and lazily retains one [`OriginCell`] per canonical
//! origin.

use super::admission::OriginAdmission;
use super::cell::OriginCell;
use super::connection::CloseReason;
use super::maintenance::{MaintenanceConfig, PartitionMaintenance};
use super::origin::{InvalidOrigin, OriginKey, OriginLookup, SchemeKey};
#[cfg(feature = "rt-tokio")]
use super::partition::TokioDriverSpawner;
use super::partition::{
    ConnectionReuseScope, DriverSpawner, EligibilityGroup, Partition, PartitionId,
};
use crate::sync::{Arc, Mutex, RwLock};
use http_1x::Uri;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::num::{NonZeroU16, NonZeroUsize};
use std::sync::{Arc as StdArc, OnceLock};

/// Fixed partition set and the origin-wide admission states it shares.
#[derive(Debug)]
pub(crate) struct PartitionRegistry {
    /// Immutable partition identities resolved when a client is built.
    partitions: HashMap<PartitionId, Arc<PartitionState>>,
    /// Policy used to derive a new cell's eligibility group.
    reuse_scope: ConnectionReuseScope,
    /// Optional origin-wide bound used when admission is first created.
    max_connections_per_host: Option<NonZeroUsize>,
    /// Admission authorities retained by canonical origin.
    bounded_origins: Mutex<HashMap<OriginKey, Arc<OriginAdmission>>>,
}

impl PartitionRegistry {
    /// Creates the anonymous partition or validates and retains explicit partitions.
    pub(crate) fn new(
        partitions: Option<Vec<Partition>>,
        reuse_scope: ConnectionReuseScope,
        max_connections_per_host: Option<NonZeroUsize>,
        maintenance: MaintenanceConfig,
    ) -> Result<Self, PartitionRegistryError> {
        let Some(partitions) = partitions else {
            let partition = Arc::new(PartitionState::anonymous(maintenance));
            let mut partitions = HashMap::new();
            partitions.insert(PartitionId::ANONYMOUS, partition);
            return Ok(Self {
                partitions,
                reuse_scope,
                max_connections_per_host,
                bounded_origins: Mutex::new(HashMap::new()),
            });
        };

        let mut by_id = HashMap::new();
        for partition in partitions {
            let id = partition.id();
            if id.is_anonymous() {
                return Err(PartitionRegistryError::ReservedAnonymousPartition);
            }
            if by_id
                .insert(
                    id,
                    Arc::new(PartitionState::explicit(partition, maintenance.clone())),
                )
                .is_some()
            {
                return Err(PartitionRegistryError::DuplicatePartition(id));
            }
        }

        if by_id.is_empty() {
            return Err(PartitionRegistryError::EmptyExplicitPartitionSet);
        }

        Ok(Self {
            partitions: by_id,
            reuse_scope,
            max_connections_per_host,
            bounded_origins: Mutex::new(HashMap::new()),
        })
    }

    /// Resolves a retained partition once during client construction.
    pub(crate) fn partition(&self, id: PartitionId) -> Option<Arc<PartitionState>> {
        self.partitions.get(&id).cloned()
    }

    /// Resolves the stable cell for an already-resolved partition and URI.
    pub(crate) fn resolve_cell(
        &self,
        partition: &PartitionState,
        uri: &Uri,
    ) -> Result<Arc<OriginCell>, InvalidOrigin> {
        let lookup = OriginLookup::from_uri(uri)?;
        if let Some(cell) = partition.find_cell(&lookup) {
            return Ok(cell);
        }

        let eligibility_group = self.eligibility_group(partition);
        let origin = lookup.into_owned();
        let admission = self.origin_admission(&origin);
        let candidate = Arc::new(OriginCell::new(
            partition.id(),
            origin,
            eligibility_group,
            admission.clone(),
            Some(partition.maintenance.clone()),
        ));
        let cell = match admission {
            Some(admission) => OriginAdmission::register_cell(&admission, candidate),
            None => candidate,
        };
        let cell = partition.publish_cell(cell);
        partition.maintenance.register(&cell);
        Ok(cell)
    }

    /// Returns the shared admission authority for a bounded origin.
    ///
    /// Unbounded registries return `None`. Concurrent first use of a bounded
    /// origin converges on one retained authority.
    fn origin_admission(&self, origin: &OriginKey) -> Option<Arc<OriginAdmission>> {
        let limit = self.max_connections_per_host?;
        let admission = {
            let mut origins = self.bounded_origins.lock();
            origins
                .entry(origin.clone())
                .or_insert_with(|| OriginAdmission::new(limit))
                .clone()
        };
        Some(admission)
    }

    /// Derives which partitions may relieve a new cell's bounded demand.
    fn eligibility_group(&self, partition: &PartitionState) -> EligibilityGroup {
        match self.reuse_scope {
            ConnectionReuseScope::Partition => EligibilityGroup::Partition(partition.id()),
            ConnectionReuseScope::NetworkInterface => {
                EligibilityGroup::NetworkInterface(partition.interface().cloned())
            }
            ConnectionReuseScope::Pool => EligibilityGroup::Pool,
        }
    }

    /// Stops partition maintenance and logically closes every connection.
    pub(crate) fn close_all(&self, reason: CloseReason) {
        for partition in self.partitions.values() {
            partition.shutdown_maintenance();
        }
        let cells = self
            .partitions
            .values()
            .flat_map(|partition| partition.cells())
            .collect::<Vec<_>>();
        for cell in cells {
            OriginCell::close_all_h1(&cell, reason);
        }
    }
}

/// Runtime placement and retained origin cells for one partition.
#[derive(Debug)]
pub(crate) struct PartitionState {
    /// Stable identity copied into every cell and owned connection.
    id: PartitionId,
    /// Configured spawner, or the first spawner published for the anonymous
    /// partition.
    spawner: OnceLock<StdArc<dyn DriverSpawner>>,
    /// Network interface used for placement and reuse eligibility.
    interface: Option<StdArc<str>>,
    /// Cells retained for the lifetime of this partition.
    origins: RwLock<OriginMap>,
    /// Owner-runtime idle maintenance for this partition.
    maintenance: Arc<PartitionMaintenance>,
}

impl PartitionState {
    /// Creates the implicit partition whose spawner is published on first use.
    fn anonymous(maintenance: MaintenanceConfig) -> Self {
        Self {
            id: PartitionId::ANONYMOUS,
            spawner: OnceLock::new(),
            interface: None,
            origins: RwLock::new(OriginMap::default()),
            maintenance: PartitionMaintenance::new(maintenance),
        }
    }

    /// Moves one validated explicit declaration into retained partition state.
    fn explicit(partition: Partition, maintenance: MaintenanceConfig) -> Self {
        let (id, spawner, interface) = partition.into_parts();
        Self {
            id,
            spawner: OnceLock::from(spawner),
            interface,
            origins: RwLock::new(OriginMap::default()),
            maintenance: PartitionMaintenance::new(maintenance),
        }
    }

    /// Returns this partition's stable identity.
    pub(crate) fn id(&self) -> PartitionId {
        self.id
    }

    /// Returns this partition's optional network-interface binding.
    pub(crate) fn interface(&self) -> Option<&StdArc<str>> {
        self.interface.as_ref()
    }

    /// Returns the runtime that owns connection drivers for this partition.
    ///
    /// Explicit partitions retain their declared spawner. The anonymous
    /// partition captures the first Tokio runtime on which it is used, and
    /// all later requests use that same runtime.
    pub(crate) fn owner_spawner(
        &self,
    ) -> Result<StdArc<dyn DriverSpawner>, MissingAnonymousRuntime> {
        if let Some(spawner) = self.spawner.get() {
            return Ok(spawner.clone());
        }

        if !self.id.is_anonymous() {
            unreachable!("an explicit partition always has a driver spawner");
        }

        #[cfg(feature = "rt-tokio")]
        {
            let handle =
                tokio::runtime::Handle::try_current().map_err(|_| MissingAnonymousRuntime)?;
            Ok(self
                .spawner
                .get_or_init(|| StdArc::new(TokioDriverSpawner::from_handle(handle)))
                .clone())
        }

        #[cfg(not(feature = "rt-tokio"))]
        {
            Err(MissingAnonymousRuntime)
        }
    }

    /// Looks up a cell without materializing an owned origin key.
    fn find_cell(&self, lookup: &OriginLookup<'_>) -> Option<Arc<OriginCell>> {
        self.origins.read().get(lookup)
    }

    /// Publishes `cell` or returns the value that won concurrent publication.
    fn publish_cell(&self, cell: Arc<OriginCell>) -> Arc<OriginCell> {
        self.origins.write().get_or_insert(cell)
    }

    /// Returns the number of cells retained by this partition.
    #[cfg(test)]
    pub(crate) fn cell_count(&self) -> usize {
        self.origins.read().len()
    }

    /// Starts idle maintenance on this partition's owner runtime.
    pub(crate) fn start_maintenance(&self, spawner: &dyn DriverSpawner) {
        PartitionMaintenance::start(&self.maintenance, spawner);
    }

    /// Stops this partition's maintenance task during pool teardown.
    fn shutdown_maintenance(&self) {
        self.maintenance.shutdown();
    }

    /// Snapshots retained cells before invoking any cell transition.
    fn cells(&self) -> Vec<Arc<OriginCell>> {
        self.origins.read().cells()
    }
}

/// Retained origin cells owned by one partition.
#[derive(Default, Debug)]
struct OriginMap {
    /// Host maps grouped by scheme and non-default port for borrowed lookup.
    indexes: HashMap<SchemePortKey, HashMap<StdArc<str>, Arc<OriginCell>>>,
    /// Cached cell total, avoiding a scan across every first-level bucket.
    cell_count: usize,
}

impl OriginMap {
    /// Resolves a cell through borrowed scheme, port, and host components.
    fn get(&self, lookup: &OriginLookup<'_>) -> Option<Arc<OriginCell>> {
        self.indexes
            .get(&SchemePortKey::from_lookup(lookup))?
            .get(lookup.host())
            .cloned()
    }

    /// Publishes one stable cell and returns an existing concurrent winner.
    fn get_or_insert(&mut self, cell: Arc<OriginCell>) -> Arc<OriginCell> {
        let lookup = OriginLookup::from_origin(cell.id().origin());
        if let Some(existing) = self.get(&lookup) {
            return existing;
        }

        let index = SchemePortKey::from_lookup(&lookup);
        let host = cell.id().origin().shared_host();
        self.indexes
            .entry(index)
            .or_default()
            .insert(host, cell.clone());
        self.cell_count = self
            .cell_count
            .checked_add(1)
            .expect("origin cell count exhausted");
        cell
    }

    /// Returns the cached number of cells across every scheme-port bucket.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.cell_count
    }

    /// Clones every retained cell without holding a lock during callbacks.
    fn cells(&self) -> Vec<Arc<OriginCell>> {
        self.indexes
            .values()
            .flat_map(|hosts| hosts.values().cloned())
            .collect()
    }
}

/// Scheme and non-default port used for the first level of origin lookup.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SchemePortKey {
    /// Canonical HTTP or HTTPS scheme.
    scheme: SchemeKey,
    /// Canonical non-default port.
    port: Option<NonZeroU16>,
}

impl SchemePortKey {
    /// Extracts the first-level index from a canonical borrowed lookup.
    fn from_lookup(lookup: &OriginLookup<'_>) -> Self {
        Self {
            scheme: lookup.scheme(),
            port: lookup.port(),
        }
    }
}

/// Error returned for an invalid explicit partition set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PartitionRegistryError {
    /// No partitions were declared.
    EmptyExplicitPartitionSet,
    /// More than one partition used the same identity.
    DuplicatePartition(PartitionId),
    /// An explicit partition used the identity reserved for the default.
    ReservedAnonymousPartition,
}

impl fmt::Display for PartitionRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyExplicitPartitionSet => {
                f.write_str("explicit partition set must not be empty")
            }
            Self::DuplicatePartition(id) => write!(f, "duplicate partition identifier: {id:?}"),
            Self::ReservedAnonymousPartition => {
                f.write_str("the anonymous partition identifier is reserved")
            }
        }
    }
}

impl Error for PartitionRegistryError {}

/// Anonymous partition use requires a runtime that can own connection tasks.
#[derive(Debug)]
pub(super) struct MissingAnonymousRuntime;

impl fmt::Display for MissingAnonymousRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "the anonymous connection-pool partition requires an active Tokio runtime on first use",
        )
    }
}

impl Error for MissingAnonymousRuntime {}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::ProtocolRequirement;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Barrier;

    #[derive(Debug)]
    struct TestSpawner;

    impl DriverSpawner for TestSpawner {
        fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
            drop(driver);
        }
    }

    fn partition(index: usize) -> Partition {
        Partition::new(PartitionId::from_index(index), TestSpawner)
    }

    #[cfg(feature = "rt-tokio")]
    fn anonymous_registry(
        reuse_scope: ConnectionReuseScope,
        max_connections_per_host: Option<NonZeroUsize>,
    ) -> PartitionRegistry {
        PartitionRegistry::new(
            None,
            reuse_scope,
            max_connections_per_host,
            MaintenanceConfig::default(),
        )
        .unwrap()
    }

    fn explicit_registry(
        partitions: impl IntoIterator<Item = Partition>,
        reuse_scope: ConnectionReuseScope,
        max_connections_per_host: Option<NonZeroUsize>,
    ) -> Result<PartitionRegistry, PartitionRegistryError> {
        PartitionRegistry::new(
            Some(partitions.into_iter().collect()),
            reuse_scope,
            max_connections_per_host,
            MaintenanceConfig::default(),
        )
    }

    fn explicit_partition(registry: &PartitionRegistry, index: usize) -> Arc<PartitionState> {
        registry
            .partition(PartitionId::from_index(index))
            .expect("test partition was not registered")
    }

    #[test]
    #[cfg(feature = "rt-tokio")]
    fn anonymous_partition_publishes_one_spawner() {
        const THREADS: usize = 8;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let registry = Arc::new(anonymous_registry(
            ConnectionReuseScope::NetworkInterface,
            None,
        ));
        let partition = registry.partition(PartitionId::ANONYMOUS).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(THREADS));
        let mut threads = Vec::new();

        for _ in 0..THREADS {
            let handle = handle.clone();
            let partition = partition.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                let _entered = handle.enter();
                barrier.wait();
                partition.owner_spawner().unwrap()
            }));
        }

        let spawners: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(spawners
            .iter()
            .skip(1)
            .all(|spawner| StdArc::ptr_eq(&spawners[0], spawner)));
    }

    #[test]
    fn explicit_registry_rejects_invalid_partition_sets() {
        assert_eq!(
            PartitionRegistryError::EmptyExplicitPartitionSet,
            explicit_registry(Vec::new(), ConnectionReuseScope::default(), None).unwrap_err()
        );
        assert_eq!(
            PartitionRegistryError::ReservedAnonymousPartition,
            explicit_registry(
                [Partition::new(PartitionId::ANONYMOUS, TestSpawner)],
                ConnectionReuseScope::default(),
                None,
            )
            .unwrap_err()
        );
        assert_eq!(
            PartitionRegistryError::DuplicatePartition(PartitionId::from_index(1)),
            explicit_registry(
                [partition(1), partition(1)],
                ConnectionReuseScope::default(),
                None,
            )
            .unwrap_err()
        );
    }

    #[test]
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
    fn registry_does_not_validate_default_connector_interface_names() {
        let registry = explicit_registry(
            [partition(1).interface("eth\0invalid")],
            ConnectionReuseScope::default(),
            None,
        )
        .unwrap();

        assert!(
            registry.partition(PartitionId::from_index(1)).is_some(),
            "transport-independent registry rejected connector-specific configuration"
        );
    }

    #[test]
    fn canonical_spellings_resolve_one_stable_cell() {
        let registry =
            explicit_registry([partition(1)], ConnectionReuseScope::default(), None).unwrap();
        let partition = explicit_partition(&registry, 1);
        let first = registry
            .resolve_cell(&partition, &"https://EXAMPLE.com:443/a".parse().unwrap())
            .unwrap();
        let owned_origin_keys = OriginLookup::owned_origin_key_materializations_for_test();
        let second = registry
            .resolve_cell(&partition, &"https://example.com/b".parse().unwrap())
            .unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            owned_origin_keys,
            OriginLookup::owned_origin_key_materializations_for_test(),
            "canonical cell hit materialized a new OriginKey"
        );
        assert_eq!(PartitionId::from_index(1), first.id().partition());
        assert_eq!("example.com", first.id().origin().host());
        assert_eq!(1, partition.cell_count());
    }

    #[test]
    fn scheme_origin_and_partition_form_cell_identity() {
        let registry = explicit_registry(
            [partition(1), partition(2)],
            ConnectionReuseScope::Pool,
            None,
        )
        .unwrap();
        let partition_1 = explicit_partition(&registry, 1);
        let partition_2 = explicit_partition(&registry, 2);
        let https: Uri = "https://example.com/".parse().unwrap();
        let http: Uri = "http://example.com/".parse().unwrap();
        let other_port: Uri = "https://example.com:8443/".parse().unwrap();

        let p1 = registry.resolve_cell(&partition_1, &https).unwrap();
        let p2 = registry.resolve_cell(&partition_2, &https).unwrap();
        let p1_http = registry.resolve_cell(&partition_1, &http).unwrap();
        let p1_other = registry.resolve_cell(&partition_1, &other_port).unwrap();

        assert!(!Arc::ptr_eq(&p1, &p2));
        assert!(!Arc::ptr_eq(&p1, &p1_http));
        assert!(!Arc::ptr_eq(&p1, &p1_other));
        assert_eq!(p1.eligibility_group(), p2.eligibility_group());
    }

    #[test]
    fn every_reuse_scope_forms_expected_groups() {
        assert_eq!(
            ConnectionReuseScope::NetworkInterface,
            ConnectionReuseScope::default()
        );
        let uri: Uri = "https://example.com/".parse().unwrap();
        let resolve = |scope| {
            let registry = explicit_registry([partition(1), partition(2)], scope, None).unwrap();
            let first_partition = explicit_partition(&registry, 1);
            let second_partition = explicit_partition(&registry, 2);
            let first = registry.resolve_cell(&first_partition, &uri).unwrap();
            let second = registry.resolve_cell(&second_partition, &uri).unwrap();
            (
                first.eligibility_group().clone(),
                second.eligibility_group().clone(),
            )
        };

        let (first, second) = resolve(ConnectionReuseScope::Partition);
        assert_ne!(first, second);
        let (first, second) = resolve(ConnectionReuseScope::NetworkInterface);
        assert_eq!(first, second);
        let (first, second) = resolve(ConnectionReuseScope::Pool);
        assert_eq!(first, second);
    }

    #[test]
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
    fn network_interface_scope_forms_exact_groups() {
        let registry = explicit_registry(
            [
                partition(1).interface("eth0"),
                partition(2).interface("eth0"),
                partition(3).interface("eth1"),
                partition(4),
            ],
            ConnectionReuseScope::NetworkInterface,
            None,
        )
        .unwrap();
        let uri: Uri = "https://example.com/".parse().unwrap();
        let cell = |id| {
            let partition = explicit_partition(&registry, id);
            registry.resolve_cell(&partition, &uri).unwrap()
        };

        assert_eq!(cell(1).eligibility_group(), cell(2).eligibility_group());
        assert_ne!(cell(1).eligibility_group(), cell(3).eligibility_group());
        assert_ne!(cell(1).eligibility_group(), cell(4).eligibility_group());
    }

    #[test]
    fn bounded_origins_share_one_admission_across_partitions() {
        let registry = explicit_registry(
            [partition(1), partition(2)],
            ConnectionReuseScope::Pool,
            NonZeroUsize::new(1),
        )
        .unwrap();
        let partition_1 = explicit_partition(&registry, 1);
        let partition_2 = explicit_partition(&registry, 2);
        let uri: Uri = "https://example.com/".parse().unwrap();
        let other: Uri = "https://other.example.com/".parse().unwrap();

        let first = registry.resolve_cell(&partition_1, &uri).unwrap();
        let second = registry.resolve_cell(&partition_2, &uri).unwrap();
        let other = registry.resolve_cell(&partition_1, &other).unwrap();

        let admission = first.admission().unwrap();
        assert!(Arc::ptr_eq(admission, second.admission().unwrap()));
        assert!(!Arc::ptr_eq(admission, other.admission().unwrap()));
    }

    #[test]
    fn unbounded_origins_construct_no_admission_state() {
        let registry = explicit_registry([partition(1)], ConnectionReuseScope::Pool, None).unwrap();
        let partition = explicit_partition(&registry, 1);
        let cell = registry
            .resolve_cell(&partition, &"https://example.com/".parse().unwrap())
            .unwrap();

        assert!(cell.admission().is_none());
        assert!(registry.bounded_origins.lock().is_empty());
    }

    #[test]
    fn first_cell_publication_is_stable_under_contention() {
        const THREADS: usize = 8;
        let registry = Arc::new(
            explicit_registry(
                [partition(1)],
                ConnectionReuseScope::default(),
                NonZeroUsize::new(1),
            )
            .unwrap(),
        );
        let partition = explicit_partition(&registry, 1);
        let barrier = Arc::new(Barrier::new(THREADS));
        let mut threads = Vec::new();

        for _ in 0..THREADS {
            let registry = registry.clone();
            let partition = partition.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                let uri: Uri = "https://example.com/".parse().unwrap();
                barrier.wait();
                registry.resolve_cell(&partition, &uri).unwrap()
            }));
        }

        let cells: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(cells
            .iter()
            .skip(1)
            .all(|cell| Arc::ptr_eq(&cells[0], cell)));
        assert_eq!(1, partition.cell_count());

        let waiter = cells[0].register_waiter(ProtocolRequirement::H1Compatible);
        let lease = OriginCell::take_ready_lease(&cells[0], waiter)
            .expect("admission targeted a different cell than the registry retained");
        drop(lease);
    }
}

#[cfg(all(test, smithy_http_client_loom))]
mod loom_tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    #[derive(Debug)]
    struct TestSpawner;

    impl DriverSpawner for TestSpawner {
        fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
            drop(driver);
        }
    }

    fn explicit_registry(
        partitions: impl IntoIterator<Item = Partition>,
        reuse_scope: ConnectionReuseScope,
        max_connections_per_host: Option<NonZeroUsize>,
    ) -> Result<PartitionRegistry, PartitionRegistryError> {
        PartitionRegistry::new(
            Some(partitions.into_iter().collect()),
            reuse_scope,
            max_connections_per_host,
            MaintenanceConfig::default(),
        )
    }

    #[test]
    fn first_cell_publication_is_stable_under_contention() {
        loom::model(|| {
            let registry = Arc::new(
                explicit_registry(
                    [Partition::new(PartitionId::from_index(1), TestSpawner)],
                    ConnectionReuseScope::default(),
                    None,
                )
                .unwrap(),
            );
            let partition = registry.partition(PartitionId::from_index(1)).unwrap();

            let first_registry = registry.clone();
            let first_partition = partition.clone();
            let first = loom::thread::spawn(move || {
                let uri: Uri = "https://example.com/".parse().unwrap();
                first_registry.resolve_cell(&first_partition, &uri).unwrap()
            });
            let second_registry = registry.clone();
            let second_partition = partition.clone();
            let second = loom::thread::spawn(move || {
                let uri: Uri = "https://example.com/".parse().unwrap();
                second_registry
                    .resolve_cell(&second_partition, &uri)
                    .unwrap()
            });

            let first = first.join().unwrap();
            let second = second.join().unwrap();
            assert!(Arc::ptr_eq(&first, &second));
            assert_eq!(1, partition.cell_count());
        });
    }
}
