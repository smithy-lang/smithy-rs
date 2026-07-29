/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Static-stability credentials caching for AWS clients.
//!
//! [`StaticStabilityCache`] is the default identity cache for AWS clients (installed by codegen and
//! `aws-config`). It is a retain-always, partition-keyed cache that provides *static stability*: on
//! a failed credential refresh it keeps serving the previously-resolved identity past expiration
//! (subject to backoff), so applications continue signing requests through a credential-source
//! outage. It caches any [`Identity`] — credentials and bearer tokens alike — and reads its
//! static-stability eligibility from a generic identity property, never downcasting to a concrete
//! credential type.
//!
//! The [`invalidation`] submodule carries the auth-failure detection half of invalidation
//! (F-INVAL-1); the cache's [`ResolveCachedIdentity::invalidate`] is the action half.

pub mod invalidation;

use aws_credential_types::provider::error::CredentialsError;
use aws_credential_types::StaticStabilityEligible;
use aws_smithy_async::future::timeout::Timeout;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_async::time::SharedTimeSource;
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

/// Timeout bounding a single credential-source resolution (inherited from `LazyCache`; converts a
/// *hung* source into a serve-cached decision). Not SEP-specified.
const DEFAULT_LOAD_TIMEOUT: Duration = Duration::from_secs(5);
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
    load_timeout: Duration,
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
        let timeout_future = sleep_impl.sleep(self.load_timeout);
        let refreshed: Result<Identity, BoxError> = match Timeout::new(
            resolver.resolve_identity(runtime_components, config_bag),
            timeout_future,
        )
        .await
        {
            // Success with fresh credentials.
            Ok(Ok(id)) if !expired(id.expiration(), now) => Ok(id),
            // An `Ok` response with expiration <= now is treated as a failed refresh (retain + back off).
            Ok(Ok(_stale)) => Err("credential source returned already-expired credentials".into()),
            Ok(Err(e)) => Err(e),
            // Timeout: converts a *hung* source into a serve-cached decision (recoverable).
            Err(_elapsed) => Err(format!(
                "credential resolution timed out after {:?}",
                self.load_timeout
            )
            .into()),
        };

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

/// SEP F-REFRESH-1 advisory window tiers, scaled by remaining credential lifetime.
fn advisory_window_for(lifetime: Duration) -> Duration {
    if lifetime <= Duration::from_secs(60 * 60) {
        Duration::from_secs(5 * 60)
    } else if lifetime <= Duration::from_secs(6 * 60 * 60) {
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

    /// Sets the timeout bounding a single credential-source resolution (default 5s).
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
            load_timeout: self.load_timeout.unwrap_or(DEFAULT_LOAD_TIMEOUT),
            mandatory_window: self.mandatory_window.unwrap_or(DEFAULT_MANDATORY_WINDOW),
            default_expiration: self.default_expiration.unwrap_or(DEFAULT_EXPIRATION),
            non_recoverable: Some(non_recoverable),
        };
        SharedIdentityCache::new(cache)
    }
}
