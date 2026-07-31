/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Static-stability credentials caching for AWS clients.
//!
//! `StaticStabilityCache` is the default identity cache for AWS clients (installed by codegen and
//! `aws-config`). It is a retain-always, partition-keyed cache that provides *static stability*: on
//! a failed credential refresh it keeps serving the previously-resolved identity past expiration
//! (subject to backoff), so applications continue signing requests through a credential-source
//! outage. It caches any `Identity` — credentials and bearer tokens alike — and reads its
//! static-stability eligibility from a generic identity property, never downcasting to a concrete
//! credential type.
//!
//! The `invalidation` submodule carries the auth-failure detection half of invalidation
//! (F-INVAL-1); the cache's `ResolveCachedIdentity::invalidate` is the action half.

pub mod invalidation;

use aws_credential_types::provider::error::CredentialsError;
use aws_credential_types::StaticStabilityEligible;
use aws_smithy_async::future::timeout::Timeout;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_async::time::SharedTimeSource;
use aws_smithy_runtime::client::identity::pessimistic_load_timeout;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::identity::{
    Identity, IdentityCachePartition, IdentityFuture, ResolveCachedIdentity, ResolveIdentity,
    SharedIdentityCache, SharedIdentityResolver,
};
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_types::config_bag::ConfigBag;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};
use tracing::Instrument;

/// SEP mandatory refresh window (blocking refresh point before expiration).
const DEFAULT_MANDATORY_WINDOW: Duration = Duration::from_secs(60);
/// Synthetic expiration for identities that don't report one (matches `LazyCache`).
const DEFAULT_EXPIRATION: Duration = Duration::from_secs(15 * 60);
/// Safety cap on cache partitions (matches `LazyCache`).
const DEFAULT_MAX_PARTITIONS: usize = 64;
/// `LazyCache`-equivalent single window for ineligible (custom/process) identities.
const LAZY_BUFFER_TIME: Duration = Duration::from_secs(10);
/// F-STABILITY-2 / C-JITTER: uniform backoff floor after a failed refresh.
const BACKOFF_MIN_SECS: u64 = 300;
/// F-STABILITY-2 / C-JITTER: uniform backoff jitter span (300..=600s total).
const BACKOFF_JITTER_SECS: u64 = 300;

/// Predicate that classifies a refresh `BoxError` as non-recoverable (terminal). Injected by the
/// AWS layer so the cache itself names no credential error types (F-FASTFAIL-1 / D-NONRECOV).
type NonRecoverablePredicate = Arc<dyn Fn(&BoxError) -> bool + Send + Sync>;

/// The default identity cache for AWS clients: retain-always, partition-keyed, static-stability.
///
/// See the [module docs](self). Build one with [`StaticStabilityCache::builder`].
pub struct StaticStabilityCache {
    partitions: RwLock<HashMap<IdentityCachePartition, Arc<Partition>>>,
    max_partitions: usize,
    // Used by `invalidate`, which receives no `RuntimeComponents`; the resolve path prefers the
    // time source from `RuntimeComponents`.
    time_source: SharedTimeSource,
    load_timeout: Option<Duration>,
    mandatory_window: Duration,
    default_expiration: Duration,
    non_recoverable: Option<NonRecoverablePredicate>,
}

impl fmt::Debug for StaticStabilityCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StaticStabilityCache")
            .field("max_partitions", &self.max_partitions)
            .field("load_timeout", &self.load_timeout)
            .field("mandatory_window", &self.mandatory_window)
            .field("default_expiration", &self.default_expiration)
            .field(
                "non_recoverable",
                &self.non_recoverable.as_ref().map(|_| "<predicate>"),
            )
            .finish()
    }
}

impl StaticStabilityCache {
    /// Returns a builder for [`StaticStabilityCache`].
    pub fn builder() -> StaticStabilityCacheBuilder {
        StaticStabilityCacheBuilder::default()
    }

    /// Get-or-create the per-source partition (honoring the SRA `IdentityCachePartition` contract).
    /// Read-mostly: the hit path takes only a shared read lock.
    fn partition(&self, key: IdentityCachePartition) -> Result<Arc<Partition>, BoxError> {
        if let Some(p) = self.partitions.read().unwrap().get(&key).cloned() {
            return Ok(p);
        }
        let mut parts = self.partitions.write().unwrap();
        if let Some(p) = parts.get(&key).cloned() {
            return Ok(p);
        }
        if parts.len() >= self.max_partitions {
            // Refuse rather than evict: a live credential partition must never be silently dropped.
            return Err(format!(
                "static stability credentials cache at max_partitions ({})",
                self.max_partitions
            )
            .into());
        }
        let p = Arc::new(Partition::default());
        parts.insert(key, p.clone());
        Ok(p)
    }

    fn snapshot_partitions(&self) -> Vec<Arc<Partition>> {
        self.partitions.read().unwrap().values().cloned().collect()
    }

    /// Refresh a partition from the source, then commit (success) or serve-cached/back-off/raise
    /// (failure). The source `.await` is held under the async `refresh_gate` only (acquired by the
    /// caller); the sync `state` lock is taken only for the brief snapshot/commit sections.
    async fn refresh(
        &self,
        part: &Partition,
        resolver: &SharedIdentityResolver,
        runtime_components: &RuntimeComponents,
        config_bag: &ConfigBag,
        now: SystemTime,
    ) -> Result<Identity, BoxError> {
        let prev = part.state.lock().unwrap().cached.clone();

        let sleep_impl = runtime_components.sleep_impl().expect("validated");
        // Match LazyCache: with no explicit timeout, derive a pessimistic one from the configured
        // retry/timeout so the source's own retries can finish before the cache kills the future.
        let load_timeout = self
            .load_timeout
            .unwrap_or_else(|| pessimistic_load_timeout(config_bag));
        let timeout_future = sleep_impl.sleep(load_timeout);
        // Mirror LazyCache's `lazy_load_identity` span so identity-load telemetry is unchanged
        // now that `StaticStabilityCache` is the default cache. Only the source call is
        // instrumented — a cache hit is served before `refresh` and never opens the span.
        let refreshed: Result<Identity, BoxError> = async move {
            match Timeout::new(
                resolver.resolve_identity(runtime_components, config_bag),
                timeout_future,
            )
            .await
            {
                // Success with fresh credentials.
                Ok(Ok(id)) if !expired(id.expiration(), now) => Ok(id),
                // An `Ok` response with expiration <= now is treated as a failed refresh (retain + back off).
                Ok(Ok(_stale)) => {
                    Err("credential source returned already-expired credentials".into())
                }
                Ok(Err(e)) => Err(e),
                // Timeout: converts a *hung* source into a serve-cached decision (recoverable).
                Err(_elapsed) => {
                    Err(format!("credential resolution timed out after {:?}", load_timeout).into())
                }
            }
        }
        .instrument(tracing::debug_span!("lazy_load_identity"))
        .await;

        match refreshed {
            Ok(id) => {
                let mut st = part.state.lock().unwrap();
                let expiry = id.expiration().unwrap_or(now + self.default_expiration);
                st.cached = Some(id.clone());
                st.expiration = Some(expiry);
                if eligible(&id) {
                    // Static-stability overlay: advisory (F-REFRESH-1) + mandatory windows.
                    st.advisory_at = Some(sub(expiry, advisory_window_for(lifetime(expiry, now))));
                    st.mandatory_at = Some(sub(expiry, self.mandatory_window));
                } else {
                    // Ineligible (custom/process): a single `LazyCache` window at expiry - buffer.
                    let at = sub(expiry, LAZY_BUFFER_TIME);
                    st.advisory_at = Some(at);
                    st.mandatory_at = Some(at);
                }
                st.next_refresh_allowed_at = None; // clear backoff
                Ok(id)
            }
            Err(err) => {
                // F-FASTFAIL-1: a non-recoverable error raises immediately — before backoff and
                // before serve-cached. The cache holds only a predicate, naming no error types.
                if self.non_recoverable.as_ref().is_some_and(|p| p(&err)) {
                    return Err(err);
                }
                let mut st = part.state.lock().unwrap();
                // Backoff AND serve-stale are BOTH static stability — gated on eligibility.
                match prev {
                    Some(c) if eligible(&c) => {
                        st.next_refresh_allowed_at = Some(now + jittered_backoff());
                        tracing::warn!(
                            error = ?err,
                            "credential refresh failed; serving cached credentials (static stability)"
                        );
                        Ok(c) // F-STABILITY-1: serve even if expired
                    }
                    // Ineligible: plain LazyCache — serve only if still valid, else raise.
                    Some(c) if !expired(st.expiration, now) => Ok(c),
                    _ => Err(err),
                }
            }
        }
    }
}

impl ResolveCachedIdentity for StaticStabilityCache {
    fn validate_base_client_config(
        &self,
        runtime_components: &RuntimeComponentsBuilder,
        _cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        validate(
            runtime_components.time_source().is_some(),
            runtime_components.sleep_impl().is_some(),
        )
    }

    fn validate_final_config(
        &self,
        runtime_components: &RuntimeComponents,
        _cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        validate(
            runtime_components.time_source().is_some(),
            runtime_components.sleep_impl().is_some(),
        )
    }

    fn resolve_cached_identity<'a>(
        &'a self,
        resolver: SharedIdentityResolver,
        runtime_components: &'a RuntimeComponents,
        config_bag: &'a ConfigBag,
    ) -> IdentityFuture<'a> {
        IdentityFuture::new(async move {
            let now = runtime_components
                .time_source()
                .map(|ts| ts.now())
                .unwrap_or_else(|| self.time_source.now());
            let part = self.partition(resolver.cache_partition())?;

            // 1) snapshot + classify under the SYNC lock — no `.await` held
            let decision = { classify(&part.state.lock().unwrap(), now) };

            match decision {
                // State 2 / backed-off: serve cached, no source contact.
                Decision::Valid(id) | Decision::RateLimited(id) => Ok(id),
                // State 3: advisory — refresh only if we win the gate, else serve cached now.
                Decision::Advisory(cached) => match part.refresh_gate.try_lock() {
                    Ok(_permit) => {
                        self.refresh(&part, &resolver, runtime_components, config_bag, now)
                            .await
                    }
                    Err(_) => Ok(cached),
                },
                // States 4 & 5 (incl. expired) and State 1 (initial): block on the gate, then
                // reuse an in-flight result if another task refreshed while we waited.
                Decision::Mandatory | Decision::Initial => {
                    let _permit = part.refresh_gate.lock().await;
                    if let Some(id) = part.recheck(now) {
                        return Ok(id);
                    }
                    // NOTE: initial-fetch rate-limiting is unspecified by the SEP (likely an
                    // oversight, raised in review); a failed first fetch just propagates.
                    self.refresh(&part, &resolver, runtime_components, config_bag, now)
                        .await
                }
            }
        })
    }

    fn invalidate(&self, rejected: &Identity) {
        let now = self.time_source.now();
        for part in self.snapshot_partitions() {
            let mut st = part.state.lock().unwrap();
            // ptr_eq generation guard: identity-agnostic (no downcast, no AKID). A served clone
            // shares the cached identity's data Arc; a concurrent refresh installs a new allocation
            // → mismatch → stale rejection is a no-op.
            if st.cached.as_ref().is_some_and(|c| c.ptr_eq(rejected)) {
                // Route the next resolution through the mandatory path. classify() keys off
                // advisory_at/mandatory_at (not expiration), so collapse both to `now`. Deliberately
                // leave next_refresh_allowed_at (backoff) and cached (static stability) untouched.
                st.expiration = Some(now);
                st.advisory_at = Some(now);
                st.mandatory_at = Some(now);
            }
        }
    }
}

/// One cache slot per credential source (per `IdentityCachePartition`).
#[derive(Debug, Default)]
struct Partition {
    /// Guards the synchronous snapshot/commit sections. NEVER held across `.await`.
    state: Mutex<CachedState>,
    /// Async single-flight gate for the source call (F-REFRESH-2). Held across `.await`.
    refresh_gate: tokio::sync::Mutex<()>,
}

impl Partition {
    /// Re-classify after acquiring the gate: another task may have refreshed while we waited.
    fn recheck(&self, now: SystemTime) -> Option<Identity> {
        let st = self.state.lock().unwrap();
        match classify(&st, now) {
            Decision::Valid(id) | Decision::RateLimited(id) => Some(id),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct CachedState {
    /// F-CACHE-1: retain-always; only ever *replaced* on success, never cleared.
    cached: Option<Identity>,
    expiration: Option<SystemTime>,
    /// F-REFRESH-1: non-blocking refresh point.
    advisory_at: Option<SystemTime>,
    /// F-REFRESH-1: blocking refresh point (<= expiration).
    mandatory_at: Option<SystemTime>,
    /// F-STABILITY-2 / C-JITTER: backoff gate after a failed refresh.
    next_refresh_allowed_at: Option<SystemTime>,
}

enum Decision {
    Valid(Identity),
    RateLimited(Identity),
    Advisory(Identity),
    Mandatory,
    Initial,
}

fn classify(st: &CachedState, now: SystemTime) -> Decision {
    let Some(id) = st.cached.clone() else {
        return Decision::Initial; // State 1
    };
    if matches!(st.advisory_at, Some(a) if now < a) {
        return Decision::Valid(id); // State 2
    }
    // Backoff gate sits AFTER Valid, BEFORE the advisory/mandatory split so an expired-but-backed-off
    // credential is served without contacting the source.
    if matches!(st.next_refresh_allowed_at, Some(t) if now < t) {
        return Decision::RateLimited(id);
    }
    match st.mandatory_at {
        Some(m) if now < m => Decision::Advisory(id), // State 3
        _ => Decision::Mandatory,                     // States 4 & 5 (includes past-expiry)
    }
}

/// D-ELIGIBLE: read the generic marker off the identity — no downcast, works for tokens too.
fn eligible(id: &Identity) -> bool {
    id.property::<StaticStabilityEligible>().is_some()
}

fn expired(expiration: Option<SystemTime>, now: SystemTime) -> bool {
    matches!(expiration, Some(exp) if exp <= now)
}

fn sub(t: SystemTime, d: Duration) -> SystemTime {
    t.checked_sub(d).unwrap_or(t)
}

fn lifetime(expiry: SystemTime, now: SystemTime) -> Duration {
    expiry.duration_since(now).unwrap_or_default()
}

/// SEP F-REFRESH-1 advisory window tiers, selected by remaining credential lifetime:
/// `<= 20min -> 5min`, `> 20min && < 90min -> 15min`, `>= 90min -> 60min`.
fn advisory_window_for(lifetime: Duration) -> Duration {
    if lifetime <= Duration::from_secs(20 * 60) {
        Duration::from_secs(5 * 60)
    } else if lifetime < Duration::from_secs(90 * 60) {
        Duration::from_secs(15 * 60)
    } else {
        Duration::from_secs(60 * 60)
    }
}

fn jittered_backoff() -> Duration {
    Duration::from_secs(BACKOFF_MIN_SECS + fastrand::u64(0..=BACKOFF_JITTER_SECS))
}

/// Default non-recoverable predicate injected into the cache by the AWS layer (D-NONRECOV): a
/// terminal `CredentialsError::Unrecoverable` bypasses backoff and static stability.
fn aws_non_recoverable(err: &BoxError) -> bool {
    err.downcast_ref::<CredentialsError>()
        .is_some_and(CredentialsError::is_unrecoverable)
}

fn validate(has_time_source: bool, has_sleep_impl: bool) -> Result<(), BoxError> {
    if !has_time_source {
        return Err("StaticStabilityCache requires a time source to be configured".into());
    }
    if !has_sleep_impl {
        return Err(
            "StaticStabilityCache requires an async sleep implementation to be configured".into(),
        );
    }
    Ok(())
}

/// Builder for [`StaticStabilityCache`].
#[derive(Clone, Debug, Default)]
pub struct StaticStabilityCacheBuilder {
    time_source: Option<SharedTimeSource>,
    load_timeout: Option<Duration>,
    mandatory_window: Option<Duration>,
    default_expiration: Option<Duration>,
    max_partitions: Option<usize>,
}

impl StaticStabilityCacheBuilder {
    /// Sets the time source used by `invalidate` (which receives no runtime components).
    pub fn time_source(mut self, time_source: SharedTimeSource) -> Self {
        self.time_source = Some(time_source);
        self
    }

    /// Sets the timeout bounding a single credential-source resolution. When unset, a pessimistic
    /// timeout is derived from the configured retry/timeout (matching `LazyCache`).
    pub fn load_timeout(mut self, load_timeout: Duration) -> Self {
        self.load_timeout = Some(load_timeout);
        self
    }

    /// Sets the mandatory (blocking) refresh window before expiration (default 60s).
    pub fn mandatory_window(mut self, window: Duration) -> Self {
        self.mandatory_window = Some(window);
        self
    }

    /// Sets the synthetic expiration for identities that don't report one (default 15m).
    pub fn default_expiration(mut self, default_expiration: Duration) -> Self {
        self.default_expiration = Some(default_expiration);
        self
    }

    /// Sets the maximum number of cache partitions (default 64).
    pub fn max_partitions(mut self, max_partitions: usize) -> Self {
        self.max_partitions = Some(max_partitions);
        self
    }

    /// Builds a [`SharedIdentityCache`] wrapping the configured [`StaticStabilityCache`].
    pub fn build(self) -> SharedIdentityCache {
        let non_recoverable: NonRecoverablePredicate = Arc::new(aws_non_recoverable);
        let cache = StaticStabilityCache {
            partitions: RwLock::new(HashMap::new()),
            max_partitions: self.max_partitions.unwrap_or(DEFAULT_MAX_PARTITIONS),
            time_source: self.time_source.unwrap_or_default(),
            load_timeout: self.load_timeout,
            mandatory_window: self.mandatory_window.unwrap_or(DEFAULT_MANDATORY_WINDOW),
            default_expiration: self.default_expiration.unwrap_or(DEFAULT_EXPIRATION),
            non_recoverable: Some(non_recoverable),
        };
        SharedIdentityCache::new(cache)
    }
}

#[cfg(test)]
mod tests {
    //! Cherry-picked scenarios from the SEP's modeled `credential-refresh-tests.json` suite,
    //! implemented idiomatically (the SEP permits this) against a `ManualTimeSource` + mock source.
    use super::*;
    use aws_smithy_async::test_util::tick_advance_sleep::tick_advance_time_and_sleep;
    use aws_smithy_async::test_util::ManualTimeSource;
    use aws_smithy_runtime_api::shared::IntoShared;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone, PartialEq)]
    struct TestCreds {
        id: u32,
    }

    fn epoch(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// Build an identity with a distinguishing id, an absolute expiration, and (optionally) the
    /// static-stability eligibility marker.
    fn identity(id: u32, expiry_secs: u64, eligible: bool) -> Identity {
        let mut b = Identity::builder()
            .data(TestCreds { id })
            .expiration(epoch(expiry_secs));
        if eligible {
            b = b.property(StaticStabilityEligible);
        }
        b.build().unwrap()
    }

    fn id_of(identity: &Identity) -> u32 {
        identity.data::<TestCreds>().unwrap().id
    }

    /// Mock credential source: returns queued results in order and counts how many times it is
    /// actually contacted (so tests can assert `sourceContacted`).
    #[derive(Debug)]
    struct MockSource {
        results: Mutex<Vec<Result<Identity, BoxError>>>,
        contacts: Arc<AtomicUsize>,
    }

    impl ResolveIdentity for MockSource {
        fn resolve_identity<'a>(
            &'a self,
            _rc: &'a RuntimeComponents,
            _cfg: &'a ConfigBag,
        ) -> IdentityFuture<'a> {
            self.contacts.fetch_add(1, Ordering::SeqCst);
            let mut list = self.results.lock().unwrap();
            let next = if list.is_empty() {
                Err("mock source: no more results".into())
            } else {
                list.remove(0)
            };
            IdentityFuture::ready(next)
        }
    }

    struct Harness {
        cache: SharedIdentityCache,
        resolver: SharedIdentityResolver,
        components: RuntimeComponents,
        config_bag: ConfigBag,
        time: ManualTimeSource,
        contacts: Arc<AtomicUsize>,
    }

    impl Harness {
        fn new(results: Vec<Result<Identity, BoxError>>) -> Self {
            let time = ManualTimeSource::new(epoch(0));
            // A tick-advance sleep never fires unless explicitly ticked, so the (immediately
            // resolving) mock source always wins the refresh Timeout race — no spurious timeouts.
            let (_tick, sleep) = tick_advance_time_and_sleep();
            let components = RuntimeComponentsBuilder::for_tests()
                .with_time_source(Some(time.clone()))
                .with_sleep_impl(Some(sleep))
                .build()
                .unwrap();
            let contacts = Arc::new(AtomicUsize::new(0));
            let resolver = SharedIdentityResolver::new(MockSource {
                results: Mutex::new(results),
                contacts: contacts.clone(),
            });
            let cache = StaticStabilityCache::builder()
                .time_source(time.clone().into_shared())
                .build();
            Self {
                cache,
                resolver,
                components,
                config_bag: ConfigBag::base(),
                time,
                contacts,
            }
        }

        /// One `getCredentials` step: returns the result and whether the source was contacted.
        async fn get(&self) -> (Result<Identity, BoxError>, bool) {
            let before = self.contacts.load(Ordering::SeqCst);
            let result = self
                .cache
                .resolve_cached_identity(self.resolver.clone(), &self.components, &self.config_bag)
                .await;
            let contacted = self.contacts.load(Ordering::SeqCst) > before;
            (result, contacted)
        }

        fn advance_to(&self, secs: u64) {
            self.time.set_time(epoch(secs));
        }
    }

    // Window selection (advisoryWindowSeconds): SEP table
    // <=20min -> 5min, >20 && <90 -> 15min, >=90 -> 60min.
    #[test]
    fn advisory_window_selection() {
        let m = |mins: u64| Duration::from_secs(mins * 60);
        assert_eq!(advisory_window_for(m(10)), m(5));
        assert_eq!(advisory_window_for(m(20)), m(5)); // boundary, inclusive
        assert_eq!(advisory_window_for(m(21)), m(15));
        assert_eq!(advisory_window_for(m(60)), m(15));
        assert_eq!(advisory_window_for(m(89)), m(15));
        assert_eq!(advisory_window_for(m(90)), m(60)); // boundary
        assert_eq!(advisory_window_for(m(6 * 60)), m(60));
    }

    // given: valid — cached credentials within neither refresh window.
    // expected: result=cachedCredentials, sourceContacted=false.
    #[tokio::test]
    async fn cached_valid_returns_cached_without_source_contact() {
        // lifetime 3600s (60min) -> advisory 15min: advisory_at=2700, mandatory_at=3540.
        let h = Harness::new(vec![Ok(identity(1, 3600, true))]);
        let (r, contacted) = h.get().await; // State 1: initial fetch
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(contacted);

        h.advance_to(1000); // < advisory_at -> State 2 valid
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(!contacted, "valid cached creds must not contact the source");
    }

    // given: advisory — within the advisory window; refresh succeeds.
    // expected: result=newCredentials, sourceContacted=true.
    #[tokio::test]
    async fn advisory_window_refreshes() {
        let h = Harness::new(vec![
            Ok(identity(1, 3600, true)),
            Ok(identity(2, 7200, true)),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3000); // 2700 <= now < 3540 -> advisory
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            2,
            "advisory window: single caller refreshes"
        );
        assert!(contacted);
    }

    // given: expired; refresh errors (recoverable). The static-stability core.
    // expected: serve cached (stale), then rate-limited (sourceContacted=false) during backoff.
    #[tokio::test]
    async fn refresh_failure_serves_cached_then_rate_limits() {
        let h = Harness::new(vec![Ok(identity(1, 3600, true)), Err("STS 503".into())]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700); // expired -> mandatory refresh -> errors
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            1,
            "eligible: serve stale on failed refresh (F-STABILITY-1)"
        );
        assert!(contacted, "a refresh was attempted");

        h.advance_to(3800); // still within backoff (>= 300s) -> rate-limited
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(
            !contacted,
            "rate-limited: must not contact source during backoff (F-STABILITY-2)"
        );
    }

    // given: expired; refresh returns a non-recoverable error.
    // expected: result=nonRecoverableError (raised, not swallowed); no serve-cached.
    #[tokio::test]
    async fn non_recoverable_error_is_raised() {
        let h = Harness::new(vec![
            Ok(identity(1, 3600, true)),
            Err(CredentialsError::unrecoverable("expired SSO token").into()),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700);
        let (r, contacted) = h.get().await;
        assert!(
            r.is_err(),
            "non-recoverable error must be raised (F-FASTFAIL-1)"
        );
        assert!(contacted);
    }

    // invalidate: the target service rejected the served identity.
    // expected: the next getCredentials takes the mandatory path and refreshes.
    #[tokio::test]
    async fn invalidate_forces_refresh_on_next_resolve() {
        let h = Harness::new(vec![
            Ok(identity(1, 3600, true)),
            Ok(identity(2, 7200, true)),
        ]);
        let served = h.get().await.0.unwrap();
        assert_eq!(id_of(&served), 1);

        h.cache.invalidate(&served); // service rejected id=1

        // now is still 0 (< the original advisory_at), but invalidate collapsed the windows.
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            2,
            "invalidation must force a refresh (F-INVAL-1)"
        );
        assert!(contacted);
    }

    // Ineligible (custom/process) identity: plain LazyCache behavior — no serve-stale.
    // expected: a failed refresh after expiry raises rather than serving stale.
    #[tokio::test]
    async fn ineligible_error_after_expiry_is_raised() {
        let h = Harness::new(vec![Ok(identity(1, 3600, false)), Err("transient".into())]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700); // expired
        let (r, _contacted) = h.get().await;
        assert!(
            r.is_err(),
            "ineligible creds must not be served past expiry on failure (F-STABILITY-3)"
        );
    }

    // given: none; the initial fetch fails.
    // expected: result=noCredentialsError (nothing cached to fall back on) — State 1.
    #[tokio::test]
    async fn initial_fetch_failure_raises() {
        let h = Harness::new(vec![Err("IMDS unreachable".into())]);
        let (r, contacted) = h.get().await;
        assert!(r.is_err(), "initial fetch failure must raise (State 1)");
        assert!(contacted);
    }

    // given: mandatory — within the mandatory window (not yet expired); refresh succeeds.
    // expected: result=newCredentials, sourceContacted=true (States 4/5, success).
    #[tokio::test]
    async fn mandatory_refresh_success() {
        let h = Harness::new(vec![
            Ok(identity(1, 3600, true)),
            Ok(identity(2, 7200, true)),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3550); // 3540 (mandatory_at) <= now < 3600 (expiry)
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 2);
        assert!(contacted);
    }

    // given: expired; the source returns Ok but with an already-past expiration (staleCredentials),
    // e.g. IMDS serving stale HOSM creds. Treated as a failed refresh.
    // expected: serve the *previous* cached creds + back off (not the stale response).
    #[tokio::test]
    async fn stale_response_treated_as_failed_refresh() {
        // second entry: id=2 but expiry 3600 (<= now once expired) -> "stale".
        let h = Harness::new(vec![
            Ok(identity(1, 3600, true)),
            Ok(identity(2, 3600, true)),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700); // both id=1 and the stale id=2 are past expiry
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            1,
            "stale Ok response is a failed refresh; serve previous"
        );
        assert!(contacted);

        h.advance_to(3800); // backoff in effect
        let (_r, contacted) = h.get().await;
        assert!(!contacted, "backoff applies after a stale response too");
    }

    // Recovery: once the backoff elapses, a later refresh is attempted and succeeds.
    #[tokio::test]
    async fn recovery_after_backoff() {
        let h = Harness::new(vec![
            Ok(identity(1, 3600, true)),
            Err("transient".into()),
            Ok(identity(2, 7200, true)),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700); // expired -> failed refresh -> serve cached + backoff (<= 600s)
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700 + 601); // past the max backoff -> refresh retried
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            2,
            "refresh retried and succeeded once backoff elapsed"
        );
        assert!(contacted);
    }

    // A source that reports no expiration gets the default expiration (15 min), so it is cached
    // and served rather than re-fetched on every call.
    #[tokio::test]
    async fn no_expiry_uses_default_expiration() {
        let no_expiry = Identity::builder()
            .data(TestCreds { id: 1 })
            .property(StaticStabilityEligible)
            .build()
            .unwrap();
        assert_eq!(no_expiry.expiration(), None);
        let h = Harness::new(vec![Ok(no_expiry)]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        // default_expiration is 15 min -> advisory window 5 min -> valid well before then.
        h.advance_to(9 * 60);
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(
            !contacted,
            "no-expiry creds are cached via default_expiration, not re-fetched"
        );
    }

    // invalidate with an identity that isn't the cached one (stale rejection / superseded by a
    // concurrent refresh) is a no-op — the ptr_eq generation guard.
    #[tokio::test]
    async fn invalidate_non_matching_is_noop() {
        let h = Harness::new(vec![Ok(identity(1, 3600, true))]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        let other = identity(99, 3600, true); // a different allocation
        h.cache.invalidate(&other);

        // still valid; the non-matching invalidate did nothing.
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(
            !contacted,
            "invalidate on a non-cached identity must be a no-op (ptr_eq guard)"
        );
    }

    // Behavioral advisory-window selection: short-lived creds (<=20 min) get the 5-min window.
    #[tokio::test]
    async fn advisory_window_short_lifetime() {
        // lifetime 900s (15 min) -> advisory 5 min (300s): advisory_at=600, mandatory_at=840.
        let h = Harness::new(vec![
            Ok(identity(1, 900, true)),
            Ok(identity(2, 7200, true)),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(500); // < 600 -> still valid
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(!contacted, "before the 5-min window: valid, no refresh");

        h.advance_to(650); // 600 <= now < 840 -> advisory window opened
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 2);
        assert!(contacted, "inside the 5-min window: refresh");
    }

    /// A source whose resolution blocks on a gate until released — for concurrency tests.
    #[derive(Debug)]
    struct GatedSource {
        gate: Arc<tokio::sync::Notify>,
        contacts: Arc<AtomicUsize>,
        results: Mutex<Vec<Result<Identity, BoxError>>>,
    }

    impl ResolveIdentity for GatedSource {
        fn resolve_identity<'a>(
            &'a self,
            _rc: &'a RuntimeComponents,
            _cfg: &'a ConfigBag,
        ) -> IdentityFuture<'a> {
            let next = self.results.lock().unwrap().remove(0);
            let (gate, contacts) = (self.gate.clone(), self.contacts.clone());
            IdentityFuture::new(async move {
                contacts.fetch_add(1, Ordering::SeqCst);
                gate.notified().await; // block until released
                next
            })
        }
    }

    // SEP Concurrency Test 1: within the advisory window, only ONE caller refreshes; the others
    // return cached immediately without waiting or contacting the source (non-blocking single-flight).
    #[tokio::test]
    async fn advisory_concurrent_single_flight() {
        let time = ManualTimeSource::new(epoch(0));
        // Tick-advance sleep: never ticked here, so the refresh Timeout never fires and a
        // gate-blocked source stays blocked until the driver releases it (deterministic, no real time).
        let (_tick, sleep) = tick_advance_time_and_sleep();
        let components = RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(time.clone()))
            .with_sleep_impl(Some(sleep))
            .build()
            .unwrap();
        let cfg = ConfigBag::base();
        let gate = Arc::new(tokio::sync::Notify::new());
        let contacts = Arc::new(AtomicUsize::new(0));
        let resolver = SharedIdentityResolver::new(GatedSource {
            gate: gate.clone(),
            contacts: contacts.clone(),
            results: Mutex::new(vec![
                Ok(identity(1, 3600, true)),
                Ok(identity(2, 7200, true)),
            ]),
        });
        let cache = StaticStabilityCache::builder()
            .time_source(time.clone().into_shared())
            .build();

        // Seed (State 1): permit exactly one source resolution for the initial fetch.
        gate.notify_one();
        let seed = cache
            .resolve_cached_identity(resolver.clone(), &components, &cfg)
            .await
            .unwrap();
        assert_eq!(id_of(&seed), 1);
        assert_eq!(contacts.load(Ordering::SeqCst), 1);

        time.set_time(epoch(3000)); // advisory window (2700 <= now < 3540)

        // Two concurrent advisory callers + a driver that releases the single in-flight refresh.
        let get1 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let get2 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let driver = async {
            tokio::task::yield_now().await;
            gate.notify_one();
        };
        let (r1, r2, ()) = tokio::join!(get1, get2, driver);

        let mut ids = [id_of(&r1.unwrap()), id_of(&r2.unwrap())];
        ids.sort();
        assert_eq!(
            ids,
            [1, 2],
            "one caller refreshed (id=2), the other served cached (id=1)"
        );
        assert_eq!(
            contacts.load(Ordering::SeqCst),
            2,
            "single-flight: only one refresh contacted the source (F-REFRESH-2)"
        );
    }

    // A failed refresh in the advisory window (creds still valid) serves cached + backs off, without
    // blocking the caller. Distinct from the mandatory/expired failure path.
    #[tokio::test]
    async fn advisory_refresh_failure_backs_off_without_blocking() {
        let h = Harness::new(vec![Ok(identity(1, 3600, true)), Err("transient".into())]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3000); // advisory window; creds still valid (exp 3600)
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            1,
            "advisory refresh failed: serve still-valid cached"
        );
        assert!(contacted, "advisory refresh was attempted");

        h.advance_to(3100); // still advisory, within backoff
        let (r, contacted) = h.get().await;
        assert_eq!(id_of(&r.unwrap()), 1);
        assert!(!contacted, "advisory backoff: no source contact");
    }

    // Behavioral advisory-window selection: assert the window opens exactly at `expiry - window`.
    async fn assert_advisory_window(lifetime_secs: u64, window_secs: u64) {
        let h = Harness::new(vec![
            Ok(identity(1, lifetime_secs, true)),
            Ok(identity(2, lifetime_secs + 100_000, true)),
        ]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1); // seed at now=0

        h.advance_to(lifetime_secs - window_secs - 1); // just before the window opens
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            1,
            "lifetime {lifetime_secs}s: valid before the {window_secs}s advisory window"
        );
        assert!(!contacted);

        h.advance_to(lifetime_secs - window_secs + 1); // just inside the window
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            2,
            "lifetime {lifetime_secs}s: refresh inside the {window_secs}s advisory window"
        );
        assert!(contacted);
    }

    #[tokio::test]
    async fn advisory_window_matrix_behavioral() {
        // SEP table, verified behaviorally through getCredentials.
        assert_advisory_window(15 * 60, 5 * 60).await; // <= 20 min  -> 5 min
        assert_advisory_window(30 * 60, 15 * 60).await; // 20..90 min -> 15 min
        assert_advisory_window(120 * 60, 60 * 60).await; // >= 90 min  -> 60 min
    }

    // SEP Concurrency Test 2: within the mandatory window / expired, one caller refreshes and all
    // others WAIT for it and reuse the result (no additional source contacts).
    #[tokio::test]
    async fn mandatory_concurrent_single_flight_all_reuse() {
        let time = ManualTimeSource::new(epoch(0));
        let (_tick, sleep) = tick_advance_time_and_sleep();
        let components = RuntimeComponentsBuilder::for_tests()
            .with_time_source(Some(time.clone()))
            .with_sleep_impl(Some(sleep))
            .build()
            .unwrap();
        let cfg = ConfigBag::base();
        let gate = Arc::new(tokio::sync::Notify::new());
        let contacts = Arc::new(AtomicUsize::new(0));
        let resolver = SharedIdentityResolver::new(GatedSource {
            gate: gate.clone(),
            contacts: contacts.clone(),
            results: Mutex::new(vec![
                Ok(identity(1, 3600, true)),
                Ok(identity(2, 10_000, true)),
            ]),
        });
        let cache = StaticStabilityCache::builder()
            .time_source(time.clone().into_shared())
            .build();

        gate.notify_one();
        let seed = cache
            .resolve_cached_identity(resolver.clone(), &components, &cfg)
            .await
            .unwrap();
        assert_eq!(id_of(&seed), 1);
        assert_eq!(contacts.load(Ordering::SeqCst), 1);

        time.set_time(epoch(3700)); // expired -> mandatory path (blocking lock + recheck)

        // Three concurrent callers + a driver that releases the single in-flight refresh.
        let get1 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let get2 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let get3 = cache.resolve_cached_identity(resolver.clone(), &components, &cfg);
        let driver = async {
            tokio::task::yield_now().await;
            gate.notify_one();
        };
        let (r1, r2, r3, ()) = tokio::join!(get1, get2, get3, driver);

        // Every caller receives the one refreshed identity; only one refresh contacted the source.
        assert_eq!(id_of(&r1.unwrap()), 2);
        assert_eq!(id_of(&r2.unwrap()), 2);
        assert_eq!(id_of(&r3.unwrap()), 2);
        assert_eq!(
            contacts.load(Ordering::SeqCst),
            2,
            "one refresh shared by all waiters (F-REFRESH-2)"
        );
    }

    // SEP §Invalidate: Invalidate() MUST NOT clear or bypass the refresh backoff. If a refresh
    // already failed and the backoff hasn't elapsed, the next getCredentials returns cached without
    // contacting the source — even after an invalidate.
    #[tokio::test]
    async fn invalidate_preserves_active_backoff() {
        let h = Harness::new(vec![Ok(identity(1, 3600, true)), Err("STS 503".into())]);
        assert_eq!(id_of(&h.get().await.0.unwrap()), 1);

        h.advance_to(3700); // expired -> failed refresh -> serve cached + backoff
        let (r, contacted) = h.get().await;
        let served = r.unwrap();
        assert_eq!(id_of(&served), 1);
        assert!(contacted);

        h.cache.invalidate(&served); // invalidate while backoff is active

        h.advance_to(3800); // still within backoff (>= 300s)
        let (r, contacted) = h.get().await;
        assert_eq!(
            id_of(&r.unwrap()),
            1,
            "invalidate must not bypass the refresh backoff"
        );
        assert!(
            !contacted,
            "backoff preserved after invalidate: no source contact"
        );
    }
}
