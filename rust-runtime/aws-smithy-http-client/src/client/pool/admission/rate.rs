/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smooths bursts of new-connection establishment into a paced ramp.
//!
//! When demand spikes — many cache-missing requests arriving at once — opening
//! every connection simultaneously makes establishment *itself* slower and more
//! failure-prone: synchronized SYNs and TLS handshakes contend for the same
//! cores and thrash the link (the thundering herd). This module paces new
//! connections to a rate `R` so they start spread out in time, and adapts `R`
//! to the rate the box can actually sustain.
//!
//! Total connection *count* is a separate concern, owned by `max_connections`.
//! This module paces the *approach* to that ceiling; it does not bound the
//! total.
//!
//! # Mechanism
//!
//! A [token bucket](TokenBucket) enforces the rate. It holds up to `B` (burst)
//! tokens and refills at `R` tokens/sec; each new connection spends one. While
//! tokens are available a connect proceeds immediately; when the bucket is empty
//! a connect waits `(1 − tokens)/R` for the next one. An idle bucket fills to
//! `B`, so a burst up to `B` passes with no delay and only sustained demand
//! above `R` is paced.
//!
//! ```text
//!   connect ──▶ token available? ──yes──▶ proceed immediately   (fast path)
//!                     │
//!                     no
//!                     ▼
//!               wait (1−tokens)/R ──▶ proceed                    (paced)
//! ```
//!
//! # Adapting the rate
//!
//! `R` is not fixed. It starts at a safe floor `R₀` and is adjusted by a
//! closed-loop [estimator] that watches realized establishment. Connect
//! completions are summarized into fixed-size [windows](WindowAgg); each window
//! feeds the estimator, which moves `R` and writes it back to the bucket:
//!
//! ```text
//!        ┌─────────────── R ───────────────┐
//!        ▼                                  │
//!   TokenBucket ──pace──▶ connect ──▶ completion (latency, ok/err)
//!                                          │
//!                                          ▼
//!                                    WindowAgg (every N)
//!                                          │
//!                                          ▼
//!                                     Estimator ─── new R ──┘
//! ```
//!
//! The estimator climbs `R` from the floor while doing so keeps raising
//! completions/sec, settles when that response flattens (the throughput knee),
//! and brakes only when latency inflates *without* a throughput gain (confirmed
//! self-saturation). It is biased to climb boldly and back off gently: for
//! establishment, undershooting the achievable rate is the costly error
//! (invisible lost throughput), while overshooting is cheap and self-correcting.
//! See [`estimator`] for the full law.
//!
//! # Cost
//!
//! Below the rate, admission is a single token check with no allocation —
//! [`ConnectRateLimit`] returns a non-boxing future. Only a connect that must
//! wait for a token takes the boxed, two-phase path. Connection *reuse* never
//! reaches this module at all (cache hits skip the connector stack).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_runtime_api::box_error::BoxError;
use pin_project_lite::pin_project;
use tokio::time::Instant;
use tower::Service;

use super::super::connection::ConnectCtx;

/// Default rate floor `R₀`, in connects/sec: the rate below which establishment
/// is never paced. Sustained demand under this rate sees no added latency.
///
/// Sized as a small fraction of one core's handshake throughput — a rate any
/// reasonable box absorbs without strain — so it is a safe "slow default" that
/// only a genuine burst exceeds.
const DEFAULT_FLOOR_PER_SEC: f64 = 100.0;

/// Default burst capacity `B`, in connects: the number admitted back-to-back,
/// without pacing, after an idle period.
///
/// This is the size of the synchronized clump the smoother permits, so it is
/// deliberately small — large enough that a client's initial handful of
/// parallel requests is not paced, small enough that pacing (and thus rate
/// discovery) engages almost immediately under a real burst.
const DEFAULT_BURST: f64 = 16.0;

/// Default ceiling on the discovered rate, in connects/sec.
///
/// A coarse backstop, not a workload limit: in normal operation the discovered
/// rate is bounded first by demand (a warm-up wants a finite number of
/// connections, then stops), by connection reuse, and by the latency brake — so
/// this value is not reached. Its job is to bound a *misbehaving* controller
/// (the climb law ignores latency while throughput rises, so a mis-measured
/// throughput could otherwise run the rate away). Set a few times above the
/// most aggressive legitimate warm-up rate observed in practice and well below
/// connection-storm magnitudes that have caused incidents.
//
// TODO: root this in a measured per-core handshake throughput. The intuition is
// that the rate maps to establishment-CPU cost (a runaway at this rate burns
// roughly `ceiling / handshakes_per_sec_per_core` cores), but the client-side
// TCP+TLS handshakes/sec/core figure is not yet measured — pending a microbench
// against the default cipher suite. Until then this is anchored to observed
// rates, not derived from CPU.
const DEFAULT_CEILING_PER_SEC: f64 = 5_000.0;

/// Default number of connect completions per estimator window.
///
/// Enough samples for a stable completions/sec and mean-latency estimate, while
/// still closing quickly under load (a higher rate fills a window faster, so
/// discovery accelerates exactly when a burst is in progress).
///
/// Must be `>=` the burst: the first window of a burst is otherwise filled
/// entirely by un-paced burst tokens, so it would never register as paced and
/// discovery would not engage.
const DEFAULT_WINDOW: u32 = 32;

/// Connection-establishment rate-limiter mode.
///
/// Selects how the pool paces new connection establishment. Defaults to
/// [`Unbounded`](Self::Unbounded) (no pacing), preserving the default behavior
/// of unlimited connection establishment.
///
/// # Variants
///
/// - **Unbounded** — connections are admitted as fast as the connection caps
///   allow.
/// - **Fixed** — a fixed token-bucket rate; never adapts.
/// - **Governed** — adaptive: the rate climbs from a floor, brakes on
///   latency inflation, and caps at a ceiling. Gain tuning (climb multiplier,
///   brake factor, etc.) uses built-in defaults; a future API may expose them.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub enum ConnectRateConfig {
    /// No pacing: connections are admitted as fast as the connection caps allow.
    #[default]
    Unbounded,
    /// Fixed token-bucket rate; never adapts.
    Fixed {
        /// Sustained connects per second.
        rate_per_sec: f64,
        /// Maximum connects admittable back-to-back after idle.
        burst: f64,
    },
    /// Adaptive: rate climbs from `floor_per_sec`, latency-braked, capped at
    /// `ceiling_per_sec`.
    Governed {
        /// Minimum rate (connects/sec); the rate never settles below this.
        floor_per_sec: f64,
        /// Maximum rate (connects/sec); a guardrail, not the discovered ceiling.
        ceiling_per_sec: f64,
        /// Maximum connects admittable back-to-back after idle.
        burst: f64,
        /// Completions per estimator window. Must be >= `burst`; clamped up if
        /// smaller.
        window_size: u32,
    },
}

impl ConnectRateConfig {
    /// Build a [`ConnectRateController`] from this configuration.
    pub(crate) fn build(&self) -> ConnectRateController {
        match *self {
            ConnectRateConfig::Unbounded => ConnectRateController::unbounded(),
            ConnectRateConfig::Fixed { rate_per_sec, burst } => {
                ConnectRateController::limited(rate_per_sec, burst)
            }
            ConnectRateConfig::Governed {
                floor_per_sec,
                ceiling_per_sec,
                burst,
                window_size,
            } => {
                // The first window must outlast the burst's free tokens for
                // discovery to engage. Clamp rather than panic so arbitrary
                // runtime config does not bring down the process.
                let min_window = burst.ceil() as u32;
                let effective_window = if window_size < min_window {
                    tracing::warn!(
                        window_size,
                        min_window,
                        "pool: connect_rate window_size below burst; clamped up to {min_window}"
                    );
                    min_window
                } else {
                    window_size
                };
                ConnectRateController::governed(
                    floor_per_sec,
                    ceiling_per_sec,
                    burst,
                    effective_window,
                )
            }
        }
    }
}

/// Pool-level governor of new-connection establishment rate.
///
/// Owns two independent pieces of state:
///
/// - a *meter* that records every successful connect completion (count and
///   summed latency). It runs in every mode, including unbounded — it is the
///   measurement of realized establishment capacity.
/// - an *actuator*, a token bucket, present only when a finite rate is set.
///   When unbounded, [`acquire`](Self::acquire) admits immediately with no
///   synchronization and no wait.
///
/// Shared across a pool's partitions via `Arc`; each partition's
/// [`ConnectRateLimit`] holds a clone.
pub(crate) struct ConnectRateController {
    meter: Meter,
    rate: Rate,
}

enum Rate {
    /// No pacing: admit immediately, take no lock, never wait.
    Unbounded,
    /// Pace admission to the bucket's refill rate. An optional governor adapts
    /// the rate from measured establishment behavior; without one the rate is
    /// fixed.
    ///
    /// The sleep that drives a pace wait is supplied per-request, not stored:
    /// pacing only happens mid-burst, when a request is in hand to supply it,
    /// and below the floor (the common case) no wait ever occurs.
    Limited { state: Mutex<LimitedState> },
}

/// The mutable state behind a limited controller. A single mutex guards the
/// bucket and the optional governor together: it is taken only on new-connect
/// establishment (never connection reuse), and `acquire` drops it before any
/// sleep — so the rate `R` is read (by `acquire`) and written (by the
/// governor on a window close) under the same lock, with no separately-shared
/// atom to reason about.
struct LimitedState {
    bucket: TokenBucket,
    governor: Option<Governor>,
}

/// The adaptive half of a limited controller: it folds windows of measured
/// behavior through the [`estimator`] and writes the resulting `R` back to the
/// bucket. Absent for a fixed-rate controller.
struct Governor {
    window: WindowAgg,
    estimator: estimator::Estimator,
}

impl ConnectRateController {
    /// A controller that never paces: every connect is admitted immediately.
    pub(crate) fn unbounded() -> Self {
        Self {
            meter: Meter::default(),
            rate: Rate::Unbounded,
        }
    }

    /// Try to admit one connect without waiting.
    ///
    /// Returns `true` if the connect may proceed immediately — the controller is
    /// unbounded, or a token was available now (the common case while demand is
    /// below the rate). Returns `false` if the connect must wait for a token, in
    /// which case nothing is consumed; the caller then waits via [`acquire`],
    /// which records the throttle.
    ///
    /// [`acquire`]: Self::acquire
    fn try_admit(&self) -> bool {
        let state = match &self.rate {
            Rate::Unbounded => return true,
            Rate::Limited { state } => state,
        };
        let mut state = state.lock().expect("connect rate state lock poisoned");
        state.bucket.try_take(Instant::now()).is_ok()
    }

    /// Admit one new connect, blocking until the rate permits it.
    ///
    /// Refills the bucket by elapsed time and takes a token if one is available;
    /// otherwise sleeps for exactly the time until the next token, then retries.
    /// The state lock is never held across the sleep. A wait marks the window as
    /// paced — evidence the rate was the binding constraint.
    ///
    /// `sleep` is the async sleep used to wait between tokens. `None` disables
    /// pacing for this request (it is admitted immediately): a limited
    /// controller cannot pace a request that carries no sleep, so it degrades to
    /// pass-through rather than block. On the SDK path a sleep is always
    /// present.
    async fn acquire(&self, sleep: Option<&SharedAsyncSleep>) {
        let state = match &self.rate {
            Rate::Unbounded => return,
            Rate::Limited { state } => state,
        };
        let Some(sleep) = sleep else {
            return;
        };
        let mut waited = Duration::ZERO;
        loop {
            let wait = {
                let mut state = state.lock().expect("connect rate state lock poisoned");
                match state.bucket.try_take(Instant::now()) {
                    Ok(()) => {
                        // Per-connect, storm-frequency: trace only, and only when
                        // this connect was actually paced.
                        if !waited.is_zero() {
                            tracing::trace!(
                                paced_for = ?waited,
                                "pool: connect paced by establishment rate"
                            );
                        }
                        return;
                    }
                    Err(wait) => {
                        if let Some(governor) = &mut state.governor {
                            governor.window.note_throttled();
                        }
                        wait
                    }
                }
            };
            waited += wait;
            sleep.sleep(wait).await;
        }
    }

    /// Record a successful connect completion and its establishment latency.
    ///
    /// Always feeds the always-on meter. When a governor is present, the sample
    /// also folds into the current window; when the window closes it advances
    /// the estimator and the new `R` is written to the bucket.
    fn observe(&self, latency: Duration) {
        self.meter.record(latency);
        let Rate::Limited { state, .. } = &self.rate else {
            return;
        };
        let mut guard = state.lock().expect("connect rate state lock poisoned");
        let LimitedState { bucket, governor } = &mut *guard;
        let Some(governor) = governor else {
            return;
        };
        if let Some(summary) = governor.window.record(Instant::now(), latency) {
            let offered_rate = governor.estimator.rate();
            // Clamp realized completions/sec to the offered rate. A window can
            // measure *faster* than the rate when the idle bucket's burst fires
            // back-to-back at the window's start (10 completions over a short
            // span), but that is one-time burst drainage, not sustained
            // capacity — you cannot sustainably establish faster than you admit.
            // Left unclamped, that inflated first sample reads the next honest
            // window as a regression and stalls the climb before it begins.
            let completions_per_sec = summary.completions_per_sec.min(offered_rate);
            let new_rate = governor.estimator.observe(estimator::Window {
                offered_rate,
                completions_per_sec,
                mean_latency: summary.mean_latency,
                paced: summary.paced,
            });
            bucket.set_rate(new_rate);
            // A rate change is a control decision — sparse (a few per storm,
            // none once settled) and the narrative of the loop, so debug. A
            // window that closed without moving the rate is routine, so trace.
            if new_rate != offered_rate {
                tracing::debug!(
                    old_rate = offered_rate,
                    new_rate,
                    completions_per_sec,
                    mean_latency = ?summary.mean_latency,
                    paced = summary.paced,
                    "pool: establishment rate adjusted"
                );
            } else {
                tracing::trace!(
                    rate = new_rate,
                    completions_per_sec,
                    mean_latency = ?summary.mean_latency,
                    paced = summary.paced,
                    "pool: establishment window closed, rate held"
                );
            }
        }
    }

    /// A controller that paces admission to a fixed `rate_per_sec`, allowing
    /// bursts of up to `burst` connects. The rate never changes.
    pub(crate) fn limited(rate_per_sec: f64, burst: f64) -> Self {
        Self {
            meter: Meter::default(),
            rate: Rate::Limited {
                state: Mutex::new(LimitedState {
                    bucket: TokenBucket::new(rate_per_sec, burst, Instant::now()),
                    governor: None,
                }),
            },
        }
    }

    /// An adaptive controller: it starts pacing at the floor `floor` and
    /// discovers the connect rate from measured establishment behavior, closing
    /// one window every `window_size` completions. `ceiling` bounds the
    /// discovered rate; `burst` is the token-bucket capacity.
    pub(crate) fn governed(floor: f64, ceiling: f64, burst: f64, window_size: u32) -> Self {
        // The first window of a burst must contain at least one paced
        // completion or discovery never engages; that requires the window to
        // outlast the burst's free tokens.
        debug_assert!(
            window_size as f64 >= burst,
            "window_size ({window_size}) must be >= burst ({burst})"
        );
        let now = Instant::now();
        let gains = estimator::Gains::defaults(floor, ceiling);
        Self {
            meter: Meter::default(),
            rate: Rate::Limited {
                state: Mutex::new(LimitedState {
                    // Start pacing at the floor; the estimator climbs from there
                    // on the first storm.
                    bucket: TokenBucket::new(floor, burst, now),
                    governor: Some(Governor {
                        window: WindowAgg::new(window_size, now),
                        estimator: estimator::Estimator::new(gains),
                    }),
                }),
            },
        }
    }

    /// An adaptive controller built from the default constants.
    // No non-test caller constructs an adaptive controller; the pool builds the
    // unbounded default.
    #[allow(dead_code)]
    pub(crate) fn governed_defaults() -> Self {
        Self::governed(
            DEFAULT_FLOOR_PER_SEC,
            DEFAULT_CEILING_PER_SEC,
            DEFAULT_BURST,
            DEFAULT_WINDOW,
        )
    }

    #[cfg(test)]
    fn current_rate(&self) -> Option<f64> {
        match &self.rate {
            Rate::Unbounded => None,
            Rate::Limited { state, .. } => Some(
                state
                    .lock()
                    .expect("connect rate state lock poisoned")
                    .bucket
                    .refill_per_sec,
            ),
        }
    }
}

/// Measurement of realized establishment: a running count of successful
/// connects and the sum of their latencies.
#[derive(Default)]
struct Meter {
    completions: AtomicU64,
    latency_sum_micros: AtomicU64,
}

impl Meter {
    fn record(&self, latency: Duration) {
        self.completions.fetch_add(1, Ordering::Relaxed);
        self.latency_sum_micros
            .fetch_add(latency.as_micros() as u64, Ordering::Relaxed);
    }
}

/// Summary of one closed window, handed to the estimator.
struct WindowSummary {
    completions_per_sec: f64,
    mean_latency: Duration,
    paced: bool,
}

/// Aggregates per-connect completions into fixed-size windows.
///
/// A window closes every `size` completions — count-based, not time-based, so
/// it needs no timer or background task and its cadence tracks load: under a
/// storm completions arrive fast and windows close fast, while in a lull a
/// window fills slowly and `R` holds. `paced` records whether the rate was the
/// binding constraint during the window (some acquire had to wait for a token),
/// distinguishing a storm from quiet demand. A window spanning a lull→storm
/// transition carries the pre-transition samples until it closes.
struct WindowAgg {
    size: u32,
    start: Instant,
    count: u32,
    latency_sum: Duration,
    throttled: u32,
}

impl WindowAgg {
    fn new(size: u32, now: Instant) -> Self {
        debug_assert!(size > 0, "window size must be positive");
        Self {
            size,
            start: now,
            count: 0,
            latency_sum: Duration::ZERO,
            throttled: 0,
        }
    }

    /// Mark that an acquire had to wait — the rate was binding this window.
    fn note_throttled(&mut self) {
        self.throttled = self.throttled.saturating_add(1);
    }

    /// Fold one completion into the current window. Returns a [`WindowSummary`]
    /// and resets when the window closes (every `size` completions).
    fn record(&mut self, now: Instant, latency: Duration) -> Option<WindowSummary> {
        self.count += 1;
        self.latency_sum += latency;
        if self.count < self.size {
            return None;
        }
        // Clamp the elapsed span to a positive floor: under a mock clock several
        // completions can land at the same instant, and a zero span would make
        // the rate non-finite.
        let elapsed = now
            .saturating_duration_since(self.start)
            .max(Duration::from_micros(1))
            .as_secs_f64();
        let summary = WindowSummary {
            completions_per_sec: self.count as f64 / elapsed,
            mean_latency: self.latency_sum / self.count,
            paced: self.throttled > 0,
        };
        self.start = now;
        self.count = 0;
        self.latency_sum = Duration::ZERO;
        self.throttled = 0;
        Some(summary)
    }
}

/// Classic token bucket: tokens refill continuously at `refill_per_sec` up to
/// `capacity`, and each admitted connect costs one token.
///
/// `refill_per_sec` (the rate) is decoupled from the token level, so it can be
/// retuned without disturbing the tokens already accumulated. `capacity` is the
/// burst: a fixed count of connects admittable back-to-back after an idle
/// period, independent of the rate.
struct TokenBucket {
    available: f64,
    capacity: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(rate_per_sec: f64, burst: f64, now: Instant) -> Self {
        debug_assert!(rate_per_sec > 0.0, "token bucket rate must be positive");
        Self {
            available: burst,
            capacity: burst,
            refill_per_sec: rate_per_sec,
            last_refill: now,
        }
    }

    /// Refill by the time elapsed since the last call, then take one token if
    /// available. On success returns `Ok(())`; otherwise returns the duration
    /// to wait until one token will have accrued.
    fn try_take(&mut self, now: Instant) -> Result<(), Duration> {
        self.refill(now);
        if self.available >= 1.0 {
            self.available -= 1.0;
            Ok(())
        } else {
            // Floor the wait at 1ns so every wait makes forward progress. The
            // ideal wait `deficit/R` can round down to 0ns — either because the
            // deficit is sub-nanosecond (a refill landed `available` just shy of
            // 1.0 through f64/Duration rounding) or because `R` is very high. A
            // zero wait that does not advance the clock would spin: the next
            // `try_take` sees the same sub-token level and recomputes the same
            // ~0 wait. One nanosecond guarantees the loop terminates.
            let deficit = 1.0 - self.available;
            let wait = Duration::from_secs_f64(deficit / self.refill_per_sec);
            Err(wait.max(Duration::from_nanos(1)))
        }
    }

    /// Credit tokens for time elapsed at the *current* rate, capped at capacity.
    fn refill(&mut self, now: Instant) {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        self.available = (self.available + elapsed * self.refill_per_sec).min(self.capacity);
        self.last_refill = now;
    }

    /// Retune the refill rate. Credits the elapsed span at the old rate first,
    /// so a rate change never retroactively re-prices past time — only future
    /// refill uses the new `R`. The token level (and thus the burst already
    /// accrued) is preserved across the change.
    fn set_rate(&mut self, rate_per_sec: f64) {
        debug_assert!(rate_per_sec > 0.0, "token bucket rate must be positive");
        self.refill(Instant::now());
        self.refill_per_sec = rate_per_sec;
    }
}

/// Paces new-connection establishment to measured establishment capacity.
///
/// Sits directly above the connect-timeout layer: any wait this layer adds is
/// outside the per-request connect timeout, which wraps only the connector call
/// below it. The connect latency measured here is therefore TCP + TLS
/// establishment, the work the inner layers perform.
pub(crate) struct ConnectRateLimit<C> {
    inner: C,
    controller: Arc<ConnectRateController>,
}

impl<C> ConnectRateLimit<C> {
    pub(crate) fn new(inner: C, controller: Arc<ConnectRateController>) -> Self {
        Self { inner, controller }
    }
}

impl<C: Clone> Clone for ConnectRateLimit<C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            controller: self.controller.clone(),
        }
    }
}

impl<C, IO> Service<ConnectCtx> for ConnectRateLimit<C>
where
    C: Service<ConnectCtx, Response = IO, Error = BoxError> + Clone + Send + 'static,
    C::Future: Send + 'static,
    IO: Send + 'static,
{
    type Response = IO;
    type Error = BoxError;
    type Future = RateFuture<C::Future, IO>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, ctx: ConnectCtx) -> Self::Future {
        if self.controller.try_admit() {
            // Fast path: admitted without waiting — the controller is unbounded,
            // or a token was available now (the common case below the rate). Run
            // the connect directly and meter it. Non-boxing.
            RateFuture::Metered {
                inner: self.inner.call(ctx),
                controller: self.controller.clone(),
                start: Instant::now(),
            }
        } else {
            // Slow path: the rate is binding and this connect must wait for a
            // token. Box the two-phase wait-then-connect. The pacing sleep rides
            // on the request; without one the controller admits immediately.
            let controller = self.controller.clone();
            let mut inner = self.inner.clone();
            let sleep = ctx.sleep_impl.clone();
            RateFuture::Paced {
                fut: Box::pin(async move {
                    controller.acquire(sleep.as_ref()).await;
                    let start = Instant::now();
                    let res = inner.call(ctx).await;
                    if res.is_ok() {
                        controller.observe(start.elapsed());
                    }
                    res
                }),
            }
        }
    }
}

pin_project! {
    /// Future for [`ConnectRateLimit`].
    ///
    /// `Metered` runs the inner connect directly and meters it — the unbounded
    /// hot path, allocation-free. `Paced` boxes the two-phase
    /// acquire-then-connect work used when a finite rate is configured.
    #[project = RateFutureProj]
    pub(crate) enum RateFuture<F, IO> {
        Metered {
            #[pin]
            inner: F,
            controller: Arc<ConnectRateController>,
            start: Instant,
        },
        Paced {
            fut: Pin<Box<dyn Future<Output = Result<IO, BoxError>> + Send>>,
        },
    }
}

impl<F, IO, E> Future for RateFuture<F, IO>
where
    F: Future<Output = Result<IO, E>>,
    E: Into<BoxError>,
{
    type Output = Result<IO, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            RateFutureProj::Metered {
                inner,
                controller,
                start,
            } => match inner.poll(cx) {
                Poll::Ready(Ok(io)) => {
                    controller.observe(start.elapsed());
                    Poll::Ready(Ok(io))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
                Poll::Pending => Poll::Pending,
            },
            RateFutureProj::Paced { fut } => fut.as_mut().poll(cx),
        }
    }
}

mod estimator {
    //! Discovers the connect rate `R` from measured establishment behavior.
    //!
    //! A pure state machine: it folds summarized [`Window`]s and returns a new
    //! `R`. Time and connect outcomes enter as data — never a clock or socket —
    //! so a given window sequence always produces the same `R` trajectory.
    //!
    //! # State machine
    //!
    //! ```text
    //!                  paced window,                  response
    //!                  response rising                flat
    //!     ┌──────┐  ───────────────────▶  ┌──────────┐ ──────▶ ┌─────────┐
    //!     │ Idle │                         │ Climbing │         │ Settled │
    //!     └──────┘  ◀───────────────────   └──────────┘         └─────────┘
    //!         ▲      throughput dropped          │ R ×= climb_mult  │
    //!         │      + latency inflated          ▼                  │ hold R
    //!         └──────────────────────────────────┴──────────────────┘
    //! ```
    //!
    //! - **Idle.** `R` rests at the floor `R₀`. An unpaced window (demand below
    //!   the rate — no storm) leaves it there. The first paced window starts a
    //!   climb.
    //! - **Climbing.** Each paced window whose completions/sec rose by at least
    //!   `min_gain` multiplies `R` by `climb_mult`, tracking the best
    //!   completions/sec and the rate that produced it. When the response
    //!   flattens, `R` settles at that best rate (stepped down by `brake_mult`
    //!   if latency had also inflated — confirmed self-saturation).
    //! - **Settled.** `R` holds at the knee. It re-enters discovery only when
    //!   completions/sec drops by `degrade_drop` *and* latency inflates — the
    //!   signature of the achievable rate having fallen (peer load, a noisy
    //!   neighbor). It does not probe upward, so a settled rate never oscillates.
    //!
    //! # Why this shape
    //!
    //! The climb signal is the *causal* one: it acts only on `R`'s effect on
    //! completions/sec, so exogenous latency (network jitter, a slow peer) that
    //! does not change throughput never triggers a backoff. Latency is used only
    //! to disambiguate a flat response — self-inflicted (brake) versus external
    //! (keep the rate). The bias toward climbing is deliberate: undershooting the
    //! achievable rate is invisible lost throughput, while overshooting it costs
    //! a little latency and corrects itself.

    use std::time::Duration;

    /// One window of measured establishment behavior, summarized for the
    /// estimator. Produced by the controller from per-connect samples; consumed
    /// here as plain data.
    #[derive(Clone, Copy, Debug)]
    pub(super) struct Window {
        /// The connect rate `R` (connects/sec) in effect during the window.
        pub(super) offered_rate: f64,
        /// Measured successful connects per second during the window.
        pub(super) completions_per_sec: f64,
        /// Mean connect (TCP + TLS) latency during the window.
        pub(super) mean_latency: Duration,
        /// Whether the rate was the binding constraint — demand exceeded `R`
        /// and connects were actually paced (a storm). When false there is no
        /// storm: discovery does not engage and the learned rate is held.
        pub(super) paced: bool,
    }

    /// Tunable gains governing the climb/brake law.
    #[derive(Clone, Copy, Debug)]
    pub(super) struct Gains {
        /// Safe default rate floor `R₀`; `R` never settles below it.
        pub(super) floor: f64,
        /// Sanity cap on `R` — a guardrail, not the discovered ceiling.
        pub(super) ceiling: f64,
        /// Multiplicative climb step (> 1).
        pub(super) climb_mult: f64,
        /// Fractional completions/sec gain required to count a window as "still
        /// climbing"; below it the response has flattened (the knee).
        pub(super) min_gain: f64,
        /// Latency above the storm's baseline by this fraction counts as
        /// inflating.
        pub(super) latency_inflation: f64,
        /// Gentle backoff factor (< 1) applied below the knee on confirmed
        /// self-inflicted saturation.
        pub(super) brake_mult: f64,
        /// Fractional drop in settled completions/sec that, with inflating
        /// latency, signals the achievable rate has fallen and discovery must
        /// resume.
        pub(super) degrade_drop: f64,
    }

    impl Gains {
        /// Default gains: a gentle multiplicative climb with a conservative,
        /// risk-asymmetric brake (back off softly, never below the floor).
        pub(super) fn defaults(floor: f64, ceiling: f64) -> Self {
            Self {
                floor,
                ceiling,
                climb_mult: 1.5,
                min_gain: 0.10,
                latency_inflation: 0.50,
                brake_mult: 0.85,
                degrade_drop: 0.25,
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Phase {
        /// No storm seen (or returned to the floor). `R` sits at the floor and
        /// the bucket is transparent; the next paced window begins discovery.
        Idle,
        /// Raising `R`, tracking the best completions/sec and the rate that
        /// produced it. `base_latency` is the ~RTT latency captured when the
        /// storm began (CPU had headroom) — the reference for detecting
        /// self-inflicted inflation.
        Climbing {
            prev_completions: f64,
            best_completions: f64,
            best_rate: f64,
            base_latency: Duration,
        },
        /// At the discovered knee. Holds `R`, watching only for downward drift
        /// (the achievable rate can fall — peer load, a noisy neighbor). Does
        /// not re-probe upward, so a settled rate never oscillates.
        Settled {
            knee_completions: f64,
            base_latency: Duration,
        },
    }

    /// Discovers and tracks the connect rate `R` from measured windows.
    pub(super) struct Estimator {
        rate: f64,
        gains: Gains,
        phase: Phase,
    }

    impl Estimator {
        pub(super) fn new(gains: Gains) -> Self {
            Self {
                rate: gains.floor,
                gains,
                phase: Phase::Idle,
            }
        }

        /// The current connect rate `R`.
        pub(super) fn rate(&self) -> f64 {
            self.rate
        }

        /// Fold one measured window into the estimate and return the new `R`.
        pub(super) fn observe(&mut self, w: Window) -> f64 {
            // No storm → discovery does not engage. A learned rate persists
            // through the lull (eviction/quiet must not wipe learning); an idle
            // floor stays at the floor.
            if !w.paced {
                return self.rate;
            }

            let g = self.gains;
            match self.phase {
                Phase::Idle => {
                    // A storm begins: capture the baseline and take the first
                    // bold climb step.
                    self.phase = Phase::Climbing {
                        prev_completions: w.completions_per_sec,
                        best_completions: w.completions_per_sec,
                        best_rate: self.rate,
                        base_latency: w.mean_latency,
                    };
                    self.set_rate(self.rate * g.climb_mult);
                }
                Phase::Climbing {
                    prev_completions,
                    best_completions,
                    best_rate,
                    base_latency,
                } => {
                    let gained = w.completions_per_sec > prev_completions * (1.0 + g.min_gain);
                    if gained {
                        // Output rose with input, so any latency rise is not our
                        // ceiling (exogenous) — ignore it and keep climbing.
                        let (best_completions, best_rate) =
                            if w.completions_per_sec > best_completions {
                                (w.completions_per_sec, w.offered_rate)
                            } else {
                                (best_completions, best_rate)
                            };
                        self.phase = Phase::Climbing {
                            prev_completions: w.completions_per_sec,
                            best_completions,
                            best_rate,
                            base_latency,
                        };
                        self.set_rate(self.rate * g.climb_mult);
                    } else {
                        // Response flattened: the knee. Settle at the best rate,
                        // stepping back from the overshoot.
                        let inflating = w.mean_latency.as_secs_f64()
                            > base_latency.as_secs_f64() * (1.0 + g.latency_inflation);
                        let settle_rate = if inflating {
                            // No throughput gain *and* latency inflated →
                            // confirmed self-saturation → back off below the knee.
                            best_rate * g.brake_mult
                        } else {
                            best_rate
                        };
                        self.set_rate(settle_rate);
                        self.phase = Phase::Settled {
                            knee_completions: best_completions,
                            base_latency,
                        };
                    }
                }
                Phase::Settled {
                    knee_completions,
                    base_latency,
                } => {
                    // Re-validate downward only: a material throughput drop with
                    // inflating latency means the ceiling fell — resume
                    // discovery from a gently reduced rate.
                    let dropped = w.completions_per_sec < knee_completions * (1.0 - g.degrade_drop);
                    let inflating = w.mean_latency.as_secs_f64()
                        > base_latency.as_secs_f64() * (1.0 + g.latency_inflation);
                    if dropped && inflating {
                        self.set_rate((self.rate * g.brake_mult).max(g.floor));
                        self.phase = Phase::Idle;
                    }
                    // Otherwise hold — stable input settles, no limit cycle.
                }
            }
            self.rate
        }

        fn set_rate(&mut self, r: f64) {
            self.rate = r.clamp(self.gains.floor, self.gains.ceiling);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn fast() -> Duration {
            Duration::from_millis(10)
        }

        /// A paced window where raising `R` keeps raising completions/sec drives
        /// `R` strictly upward.
        #[test]
        fn climbs_on_rising_response() {
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            let mut last = est.rate();
            for _ in 0..6 {
                let r = est.rate();
                // completions track the offered rate (below any knee).
                let new_r = est.observe(Window {
                    offered_rate: r,
                    completions_per_sec: r,
                    mean_latency: fast(),
                    paced: true,
                });
                assert!(new_r >= last, "rate must not fall while climbing");
                last = new_r;
            }
            assert!(last > 10.0 * 3.0, "several multiplicative steps: {last}");
        }

        /// When the completions/sec response flattens, `R` stops climbing and
        /// settles near the knee (no further movement on stable input).
        #[test]
        fn settles_at_knee_on_flat_response() {
            let knee = 50.0;
            let plant = |r: f64| r.min(knee);
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            let mut trajectory = Vec::new();
            for _ in 0..20 {
                let r = est.rate();
                est.observe(Window {
                    offered_rate: r,
                    completions_per_sec: plant(r),
                    mean_latency: fast(),
                    paced: true,
                });
                trajectory.push(est.rate());
            }
            let settled = est.rate();
            assert!(
                settled >= knee && settled <= knee * 1.6,
                "settled near knee: {settled}"
            );
            let tail = &trajectory[trajectory.len() - 5..];
            assert!(
                tail.iter().all(|&x| (x - settled).abs() < 1e-9),
                "no movement once settled: {tail:?}"
            );
        }

        /// A flat response *with* inflated latency is confirmed self-saturation:
        /// `R` backs off below the best rate, but never below the floor.
        #[test]
        fn brakes_on_confirmed_saturation() {
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            // Two gaining windows establish a best rate of 15.
            est.observe(Window {
                offered_rate: 10.0,
                completions_per_sec: 10.0,
                mean_latency: fast(),
                paced: true,
            });
            est.observe(Window {
                offered_rate: 15.0,
                completions_per_sec: 15.0,
                mean_latency: fast(),
                paced: true,
            });
            // Flat response + inflated latency.
            let after = est.observe(Window {
                offered_rate: est.rate(),
                completions_per_sec: 15.0,
                mean_latency: Duration::from_millis(40),
                paced: true,
            });
            assert!(after < 15.0, "braked below knee best rate: {after}");
            assert!(after >= 10.0, "never below the floor: {after}");
        }

        /// Latency that inflates while the throughput response still rises is
        /// exogenous; the estimator keeps climbing (the causal discipline).
        #[test]
        fn ignores_exogenous_latency() {
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            est.observe(Window {
                offered_rate: 10.0,
                completions_per_sec: 10.0,
                mean_latency: fast(),
                paced: true,
            });
            let before = est.rate();
            let after = est.observe(Window {
                offered_rate: before,
                completions_per_sec: 30.0, // response still rising
                mean_latency: Duration::from_millis(100), // but latency inflated
                paced: true,
            });
            assert!(
                after > before,
                "rising response keeps the climb despite latency: {after} !> {before}"
            );
        }

        /// Unpaced windows (no storm) never move `R` off the floor.
        #[test]
        fn floored_without_a_storm() {
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            for _ in 0..5 {
                est.observe(Window {
                    offered_rate: 10.0,
                    completions_per_sec: 5.0,
                    mean_latency: fast(),
                    paced: false,
                });
            }
            assert_eq!(est.rate(), 10.0);
        }

        /// `R` is clamped to the sanity ceiling no matter how long it gains.
        #[test]
        fn capped_at_ceiling() {
            let ceiling = 100.0;
            let mut est = Estimator::new(Gains::defaults(10.0, ceiling));
            for _ in 0..50 {
                let r = est.rate();
                est.observe(Window {
                    offered_rate: r,
                    completions_per_sec: r * 2.0,
                    mean_latency: fast(),
                    paced: true,
                });
                assert!(est.rate() <= ceiling, "never exceeds ceiling");
            }
        }

        /// Stable input after settling produces no oscillation.
        #[test]
        fn stable_input_no_limit_cycle() {
            let plant = |r: f64| r.min(50.0);
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            for _ in 0..15 {
                let r = est.rate();
                est.observe(Window {
                    offered_rate: r,
                    completions_per_sec: plant(r),
                    mean_latency: fast(),
                    paced: true,
                });
            }
            let settled = est.rate();
            let mut tail = Vec::new();
            for _ in 0..20 {
                let r = est.rate();
                est.observe(Window {
                    offered_rate: r,
                    completions_per_sec: plant(r),
                    mean_latency: fast(),
                    paced: true,
                });
                tail.push(est.rate());
            }
            assert!(
                tail.iter().all(|&x| (x - settled).abs() < 1e-9),
                "settled rate must not cycle: {tail:?}"
            );
        }

        /// A learned rate survives a lull: unpaced windows hold `R` rather than
        /// resetting it (eviction/quiet must not wipe learning).
        #[test]
        fn lull_holds_learned_rate() {
            let plant = |r: f64| r.min(50.0);
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            for _ in 0..15 {
                let r = est.rate();
                est.observe(Window {
                    offered_rate: r,
                    completions_per_sec: plant(r),
                    mean_latency: fast(),
                    paced: true,
                });
            }
            let learned = est.rate();
            assert!(learned > 10.0, "must have climbed above the floor first");
            for _ in 0..5 {
                est.observe(Window {
                    offered_rate: learned,
                    completions_per_sec: 0.0,
                    mean_latency: fast(),
                    paced: false,
                });
            }
            assert_eq!(est.rate(), learned, "lull must not wipe the learned rate");
        }

        /// Once settled, a material throughput drop with inflating latency means
        /// the ceiling fell: discovery resumes from a reduced rate.
        #[test]
        fn revalidates_downward_on_degradation() {
            let plant = |r: f64| r.min(50.0);
            let mut est = Estimator::new(Gains::defaults(10.0, 10_000.0));
            for _ in 0..15 {
                let r = est.rate();
                est.observe(Window {
                    offered_rate: r,
                    completions_per_sec: plant(r),
                    mean_latency: fast(),
                    paced: true,
                });
            }
            let knee = est.rate();
            let after = est.observe(Window {
                offered_rate: knee,
                completions_per_sec: 10.0, // collapsed throughput
                mean_latency: Duration::from_millis(60), // inflated latency
                paced: true,
            });
            assert!(after < knee, "ceiling fell → R drops: {after} !< {knee}");
            assert!(after >= 10.0, "never below the floor");
        }
    }
}

#[cfg(test)]
mod plant_sim {
    //! Closed-loop plant-model simulator.
    //!
    //! The estimator's own unit tests are *open-loop*: they feed scripted
    //! windows and assert the response. This closes the loop — a modeled box
    //! produces each window from the rate the estimator just chose, so the
    //! estimator drives its own input — to check that the law converges and
    //! where it settles.
    //!
    //! The plant models an establishment knee: connect throughput saturates at
    //! `knee` connects/sec, and connect latency holds at the base RTT until
    //! offered load passes the knee, then grows with the overload ratio
    //! (shared-CPU contention).
    //!
    //! These tests assert the loop is stable (converges, no runaway, no
    //! collapse) and near-optimal (captures most of the knee). They do not pin
    //! exact constants: a multiplicative climb overshoots the knee by up to
    //! `climb_mult`, and the latency that overshoot creates can trip the brake,
    //! so the settled rate depends on where the climb grid falls relative to the
    //! knee — a property of the gains, not of a fixed operating point.

    use super::estimator::{Estimator, Gains, Window};
    use std::time::Duration;

    /// A modeled box. Throughput saturates at `knee`; latency is flat below it
    /// and inflates with the overload ratio above it.
    struct Plant {
        knee: f64,
        base_rtt: Duration,
    }

    impl Plant {
        /// The (completions/sec, mean latency) the box yields for an offered
        /// rate `r`. `completions = min(r, knee)`; latency inflates only once
        /// `r` exceeds the knee.
        fn respond(&self, r: f64) -> (f64, Duration) {
            let completions = r.min(self.knee);
            let overload = (r / self.knee).max(1.0);
            (completions, self.base_rtt.mul_f64(overload))
        }
    }

    /// Drive the closed loop for `steps` under a sustained storm
    /// (`paced = true`). Returns `(rate, completions/sec)` per step.
    fn run(est: &mut Estimator, plant: &Plant, steps: usize) -> Vec<(f64, f64)> {
        let mut trace = Vec::with_capacity(steps);
        for _ in 0..steps {
            let r = est.rate();
            let (completions, latency) = plant.respond(r);
            est.observe(Window {
                offered_rate: r,
                completions_per_sec: completions,
                mean_latency: latency,
                paced: true,
            });
            trace.push((est.rate(), completions));
        }
        trace
    }

    /// The loop converges: the rate stops moving within a tight band over the
    /// tail of a long run.
    #[test]
    fn converges_to_a_stable_rate() {
        let plant = Plant {
            knee: 200.0,
            base_rtt: Duration::from_millis(10),
        };
        let mut est = Estimator::new(Gains::defaults(10.0, 100_000.0));
        let trace = run(&mut est, &plant, 60);
        let tail = &trace[trace.len() - 10..];
        let first = tail[0].0;
        assert!(
            tail.iter().all(|&(r, _)| (r - first).abs() < first * 1e-6),
            "rate must be stable over the tail: {:?}",
            tail.iter().map(|&(r, _)| r).collect::<Vec<_>>()
        );
    }

    /// The settled operating point captures the majority of the knee's
    /// throughput, and does not sit far above the knee pumping latency for no
    /// throughput gain (no sustained overshoot).
    #[test]
    fn settles_near_and_below_the_knee() {
        let knee = 200.0;
        let plant = Plant {
            knee,
            base_rtt: Duration::from_millis(10),
        };
        let mut est = Estimator::new(Gains::defaults(10.0, 100_000.0));
        run(&mut est, &plant, 60);
        let settled = est.rate();
        let (captured, _) = plant.respond(settled);
        // Near-optimal: most of the knee's achievable throughput is captured.
        assert!(
            captured >= 0.8 * knee,
            "captured {captured}/sec of a {knee}/sec knee"
        );
        // No sustained overshoot: it does not park well above the knee, where
        // extra rate only buys latency.
        assert!(
            settled <= 1.2 * knee,
            "settled {settled} should not sit far above the knee {knee}"
        );
    }

    /// The floor sets the *starting* point of discovery, not the destination:
    /// from any `R₀` the loop converges into the near-knee band, so floor
    /// calibration trades discovery cost (a longer or shorter climb), not the
    /// operating point.
    ///
    /// It does *not* converge to the same exact rate from every floor. The
    /// multiplicative climb visits a different rate-grid depending on the floor
    /// (e.g. 5·1.5ⁿ vs 80·1.5ⁿ), and whether an overshoot step trips the brake
    /// depends on where that grid falls relative to the knee — so the settle
    /// points differ (~288 vs ~344 for a 300/sec knee) while both stay
    /// near-optimal. The invariant is the band, not a single point.
    #[test]
    fn floor_sets_start_not_destination() {
        let knee = 300.0;
        let plant = Plant {
            knee,
            base_rtt: Duration::from_millis(10),
        };
        let settle_from = |floor: f64| {
            let mut est = Estimator::new(Gains::defaults(floor, 100_000.0));
            run(&mut est, &plant, 80);
            est.rate()
        };
        for floor in [5.0, 20.0, 80.0, 150.0] {
            let settled = settle_from(floor);
            assert!(
                settled >= 0.8 * knee && settled <= 1.2 * knee,
                "from floor {floor}, settled {settled} should be in the near-knee \
                 band [{}, {}]",
                0.8 * knee,
                1.2 * knee
            );
        }
    }

    /// A storm that subsides (paced → unpaced) holds the discovered rate, and a
    /// second storm resumes from that warm prior rather than re-climbing from
    /// the floor — eviction/quiet must not wipe learning.
    #[test]
    fn second_storm_resumes_from_warm_prior() {
        let knee = 200.0;
        let plant = Plant {
            knee,
            base_rtt: Duration::from_millis(10),
        };
        let mut est = Estimator::new(Gains::defaults(10.0, 100_000.0));
        run(&mut est, &plant, 60);
        let learned = est.rate();
        // A lull: demand falls below the floor, no pacing.
        for _ in 0..10 {
            est.observe(Window {
                offered_rate: learned,
                completions_per_sec: 0.0,
                mean_latency: plant.base_rtt,
                paced: false,
            });
        }
        assert_eq!(
            est.rate(),
            learned,
            "the lull must not wipe the learned rate"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};

    /// A full bucket admits `burst` connects back-to-back, then the next take
    /// reports the wait until one token refills.
    #[tokio::test(start_paused = true)]
    async fn bucket_drains_burst_then_reports_wait() {
        let now = Instant::now();
        let mut bucket = TokenBucket::new(10.0, 3.0, now);
        assert!(bucket.try_take(now).is_ok());
        assert!(bucket.try_take(now).is_ok());
        assert!(bucket.try_take(now).is_ok());
        // Empty now: at 10/s the next token is 100ms away.
        let wait = bucket.try_take(now).expect_err("bucket should be empty");
        assert_eq!(wait, Duration::from_millis(100));
    }

    /// After enough elapsed time a token is available again.
    #[tokio::test(start_paused = true)]
    async fn bucket_refills_over_time() {
        let now = Instant::now();
        let mut bucket = TokenBucket::new(10.0, 1.0, now);
        assert!(bucket.try_take(now).is_ok());
        assert!(bucket.try_take(now).is_err());
        // 100ms later, exactly one token has accrued.
        let later = now + Duration::from_millis(100);
        assert!(bucket.try_take(later).is_ok());
    }

    /// Refill is clamped to capacity: idle time never accrues more than `burst`
    /// tokens.
    #[tokio::test(start_paused = true)]
    async fn bucket_refill_clamps_to_capacity() {
        let now = Instant::now();
        let mut bucket = TokenBucket::new(10.0, 2.0, now);
        // Drain.
        assert!(bucket.try_take(now).is_ok());
        assert!(bucket.try_take(now).is_ok());
        // Idle a long time, far more than capacity/rate.
        let later = now + Duration::from_secs(100);
        assert!(bucket.try_take(later).is_ok());
        assert!(bucket.try_take(later).is_ok());
        // Only `burst` were available despite the long idle.
        assert!(bucket.try_take(later).is_err());
    }

    /// An empty bucket always reports a strictly-positive wait, even at a high
    /// rate where the ideal wait would round down to zero. A zero wait that does
    /// not advance the clock would spin the `acquire` loop forever.
    #[tokio::test(start_paused = true)]
    async fn empty_bucket_wait_is_never_zero() {
        let now = Instant::now();
        // At 10_000/s the ideal inter-token gap is 100µs, but after a refill
        // leaves `available` a hair under 1.0 the residual deficit can round to
        // 0ns; the floor must keep it positive.
        let mut bucket = TokenBucket::new(10_000.0, 1.0, now);
        assert!(bucket.try_take(now).is_ok()); // burst token
        let wait = bucket.try_take(now).expect_err("empty");
        assert!(
            wait > Duration::ZERO,
            "wait must make forward progress: {wait:?}"
        );
        // Advancing by the reported wait then makes a token available.
        assert!(bucket.try_take(now + wait).is_ok());
    }

    fn test_sleep() -> SharedAsyncSleep {
        SharedAsyncSleep::new(TokioSleep::new())
    }

    /// The default constants are internally consistent: ordered floor < ceiling
    /// and window >= burst (so the first window of a burst registers as paced
    /// and discovery engages). `governed_defaults` also asserts the latter.
    #[test]
    fn default_constants_are_consistent() {
        assert!(DEFAULT_FLOOR_PER_SEC < DEFAULT_CEILING_PER_SEC);
        assert!(DEFAULT_BURST >= 1.0);
        assert!(
            DEFAULT_WINDOW as f64 >= DEFAULT_BURST,
            "window ({DEFAULT_WINDOW}) must be >= burst ({DEFAULT_BURST})"
        );
        // Constructs without tripping the window >= burst debug assertion.
        let _ = ConnectRateController::governed_defaults();
    }

    /// An unbounded controller admits immediately and takes no lock, with or
    /// without a sleep.
    #[tokio::test(start_paused = true)]
    async fn unbounded_admits_immediately() {
        let controller = ConnectRateController::unbounded();
        // Completes without advancing the (paused) clock.
        controller.acquire(None).await;
    }

    /// `try_admit` is the predicate that decides the non-boxing fast path: it is
    /// true for an unbounded controller, true while a token is available (the
    /// common case below the rate), and false once the bucket is empty.
    #[tokio::test(start_paused = true)]
    async fn try_admit_tracks_token_availability() {
        let unbounded = ConnectRateController::unbounded();
        assert!(unbounded.try_admit(), "unbounded always admits");

        // Burst of 2: two immediate admits, then the bucket is empty.
        let limited = ConnectRateController::limited(10.0, 2.0);
        assert!(limited.try_admit(), "first burst token");
        assert!(limited.try_admit(), "second burst token");
        assert!(
            !limited.try_admit(),
            "empty bucket → must wait, no fast path"
        );
    }

    /// A limited controller with no sleep cannot pace, so it degrades to
    /// admitting immediately rather than blocking.
    #[tokio::test(start_paused = true)]
    async fn limited_without_sleep_admits_immediately() {
        let controller = ConnectRateController::limited(10.0, 1.0);
        let start = Instant::now();
        // Drain the single burst token, then several more that would pace if a
        // sleep were present.
        for _ in 0..5 {
            controller.acquire(None).await;
        }
        assert_eq!(start.elapsed(), Duration::ZERO, "no sleep → no pacing");
    }

    /// A limited controller paces: the first acquire is immediate (burst), the
    /// second blocks until the next token refills.
    #[tokio::test(start_paused = true)]
    async fn limited_paces_after_burst() {
        let sleep = test_sleep();
        let controller = ConnectRateController::limited(10.0, 1.0);
        let start = Instant::now();
        controller.acquire(Some(&sleep)).await; // burst token, immediate
        assert_eq!(start.elapsed(), Duration::ZERO);
        controller.acquire(Some(&sleep)).await; // must wait ~100ms for refill
        assert_eq!(start.elapsed(), Duration::from_millis(100));
    }

    /// A fixed-rate controller (no governor) never moves its rate, however many
    /// completions it observes.
    #[tokio::test(start_paused = true)]
    async fn fixed_rate_does_not_adapt() {
        let controller = ConnectRateController::limited(10.0, 100.0);
        for _ in 0..100 {
            controller.observe(Duration::from_millis(5));
        }
        assert_eq!(controller.current_rate(), Some(10.0));
    }

    /// End-to-end closed loop through the real `acquire`/`observe` path: under a
    /// sustained storm a governed controller drives its rate up off the floor.
    #[tokio::test(start_paused = true)]
    async fn governed_climbs_under_a_storm() {
        let sleep = test_sleep();
        // Floor 20/s, generous ceiling, burst 5, close a window every 10
        // completions.
        let controller = ConnectRateController::governed(20.0, 100_000.0, 5.0, 10);
        assert_eq!(controller.current_rate(), Some(20.0));
        // Drive far more demand than the floor permits: each acquire paces, each
        // completion folds into the window. The connector is modeled as keeping
        // up (latency flat), so the response keeps rising and the rate climbs.
        for _ in 0..200 {
            controller.acquire(Some(&sleep)).await;
            controller.observe(Duration::from_millis(5));
        }
        let rate = controller
            .current_rate()
            .expect("governed controller is limited");
        assert!(
            rate > 20.0,
            "a sustained storm must climb the rate off the floor: {rate}"
        );
    }

    /// Below the floor there is no storm: a governed controller admits a burst
    /// and steady sub-floor demand without ever pacing, and holds its rate at
    /// the floor (transparent — the always-on safety net).
    #[tokio::test(start_paused = true)]
    async fn governed_transparent_below_floor() {
        let sleep = test_sleep();
        // Floor 1000/s, burst 50, window >= burst per the invariant.
        let controller = ConnectRateController::governed(1000.0, 100_000.0, 50.0, 50);
        let start = Instant::now();
        // A burst within capacity, then steady demand well under the floor.
        for _ in 0..50 {
            controller.acquire(Some(&sleep)).await;
            controller.observe(Duration::from_millis(1));
        }
        // No pacing happened: the burst fit and demand stayed sub-floor.
        assert_eq!(
            start.elapsed(),
            Duration::ZERO,
            "sub-floor demand must not pace"
        );
        // Rate holds at the floor: an unpaced window never engages discovery.
        assert_eq!(controller.current_rate(), Some(1000.0));
    }

    /// `ConnectRateConfig::default()` is `Unbounded`.
    #[test]
    fn config_default_is_unbounded() {
        assert!(matches!(
            ConnectRateConfig::default(),
            ConnectRateConfig::Unbounded
        ));
    }

    /// Building from `Unbounded` produces a controller that admits immediately
    /// (no rate state).
    #[tokio::test(start_paused = true)]
    async fn config_unbounded_builds_unbounded_controller() {
        let controller = ConnectRateConfig::Unbounded.build();
        assert!(controller.current_rate().is_none(), "unbounded has no rate");
        assert!(controller.try_admit(), "unbounded always admits");
    }

    /// Building from `Fixed` produces a limited controller at the configured
    /// rate.
    #[tokio::test(start_paused = true)]
    async fn config_fixed_builds_limited_controller() {
        let controller = ConnectRateConfig::Fixed {
            rate_per_sec: 50.0,
            burst: 2.0,
        }
        .build();
        assert_eq!(
            controller.current_rate(),
            Some(50.0),
            "fixed controller has the configured rate"
        );
        // Burst of 2 then empty.
        assert!(controller.try_admit());
        assert!(controller.try_admit());
        assert!(!controller.try_admit(), "burst exhausted");
    }

    /// Building from `Governed` produces an adaptive (limited) controller at
    /// the floor rate.
    #[tokio::test(start_paused = true)]
    async fn config_governed_builds_governed_controller() {
        let controller = ConnectRateConfig::Governed {
            floor_per_sec: 25.0,
            ceiling_per_sec: 5000.0,
            burst: 8.0,
            window_size: 16,
        }
        .build();
        assert_eq!(
            controller.current_rate(),
            Some(25.0),
            "governed starts at the floor"
        );
    }

    /// `Governed` with `window_size < burst` clamps `window_size` upward
    /// instead of panicking.
    #[test]
    fn config_governed_clamps_window_below_burst() {
        // burst=10, window_size=5 → should clamp to 10 without panic.
        let controller = ConnectRateConfig::Governed {
            floor_per_sec: 20.0,
            ceiling_per_sec: 1000.0,
            burst: 10.0,
            window_size: 5,
        }
        .build();
        // The controller constructed successfully (no panic). It should be at
        // the floor rate, confirming it is governed/limited.
        assert_eq!(controller.current_rate(), Some(20.0));
    }
}

/// `ConnectRateLimit` exercised in isolation as a tower layer, over a mock
/// inner connector — verifying the layer's contract (pass-through, pacing,
/// readiness, error propagation) independently of any real connection.
#[cfg(test)]
mod layer_tests {
    use super::*;
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    use super::super::super::connection::ConnectCtx;

    /// A cloneable mock connector that counts calls and returns a configurable
    /// outcome after a configurable delay. Stands in for the TCP+TLS connector
    /// so the layer can be driven without a socket.
    #[derive(Clone)]
    struct MockConnector {
        calls: Arc<AtomicUsize>,
        delay: Duration,
        fail: bool,
    }

    impl MockConnector {
        fn ok() -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                delay: Duration::ZERO,
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                fail: true,
                delay: Duration::ZERO,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Service<ConnectCtx> for MockConnector {
        type Response = ();
        type Error = BoxError;
        type Future = Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _ctx: ConnectCtx) -> Self::Future {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let delay = self.delay;
            let fail = self.fail;
            Box::pin(async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                if fail {
                    Err("mock connector failure".into())
                } else {
                    Ok(())
                }
            })
        }
    }

    fn ctx_with_sleep(sleep: &SharedAsyncSleep) -> ConnectCtx {
        ConnectCtx::new("http://example.com".parse().unwrap(), None).with_sleep(Some(sleep.clone()))
    }

    /// An unbounded layer forwards every call straight to the inner connector,
    /// unchanged.
    #[tokio::test(start_paused = true)]
    async fn unbounded_passes_through_to_inner() {
        let inner = MockConnector::ok();
        let mut svc =
            ConnectRateLimit::new(inner.clone(), Arc::new(ConnectRateController::unbounded()));
        let ctx = ConnectCtx::new("http://example.com".parse().unwrap(), None);
        svc.ready().await.unwrap().call(ctx).await.unwrap();
        assert_eq!(inner.calls(), 1, "the connector ran exactly once");
    }

    /// `poll_ready` reflects the inner connector's readiness (it is forwarded,
    /// not shadowed by the layer).
    #[tokio::test(start_paused = true)]
    async fn poll_ready_forwards_to_inner() {
        let inner = MockConnector::ok();
        let mut svc =
            ConnectRateLimit::new(inner, Arc::new(ConnectRateController::governed_defaults()));
        // The default mock is always ready; the layer must surface that.
        std::future::poll_fn(|cx| svc.poll_ready(cx)).await.unwrap();
    }

    /// A connector error propagates through the layer unchanged, and a failed
    /// connect is not metered as a completion.
    #[tokio::test(start_paused = true)]
    async fn inner_error_propagates_and_is_not_metered() {
        let inner = MockConnector::failing();
        let controller = Arc::new(ConnectRateController::governed_defaults());
        let mut svc = ConnectRateLimit::new(inner.clone(), controller.clone());
        let sleep = SharedAsyncSleep::new(TokioSleep::new());
        let ctx = ctx_with_sleep(&sleep);
        let err = svc.ready().await.unwrap().call(ctx).await.unwrap_err();
        assert!(format!("{err}").contains("mock connector failure"));
        assert_eq!(inner.calls(), 1);
        // A failed connect must not count toward the realized-rate meter.
        assert_eq!(controller.meter.completions.load(Ordering::Relaxed), 0);
    }

    /// Under a limited layer, demand beyond the burst is paced: the connector is
    /// still called once per request, but later calls are delayed by the rate.
    #[tokio::test(start_paused = true)]
    async fn limited_paces_calls_to_inner() {
        let inner = MockConnector::ok();
        // 10/s, burst of 1: the first call is immediate, the second waits ~100ms.
        let controller = Arc::new(ConnectRateController::limited(10.0, 1.0));
        let svc = ConnectRateLimit::new(inner.clone(), controller);
        let sleep = SharedAsyncSleep::new(TokioSleep::new());

        let start = Instant::now();
        svc.clone()
            .ready()
            .await
            .unwrap()
            .call(ctx_with_sleep(&sleep))
            .await
            .unwrap();
        assert_eq!(start.elapsed(), Duration::ZERO, "first call: burst token");

        svc.clone()
            .ready()
            .await
            .unwrap()
            .call(ctx_with_sleep(&sleep))
            .await
            .unwrap();
        assert_eq!(
            start.elapsed(),
            Duration::from_millis(100),
            "second call paced by the rate"
        );
        assert_eq!(inner.calls(), 2, "both calls reached the connector");
    }

    /// A limited layer whose request carries no sleep cannot pace, so it admits
    /// immediately — the connector still runs, just without rate enforcement.
    #[tokio::test(start_paused = true)]
    async fn limited_without_sleep_passes_through() {
        let inner = MockConnector::ok();
        let controller = Arc::new(ConnectRateController::limited(10.0, 1.0));
        let svc = ConnectRateLimit::new(inner.clone(), controller);

        let start = Instant::now();
        for _ in 0..5 {
            // No sleep on the ctx → no pacing.
            let ctx = ConnectCtx::new("http://example.com".parse().unwrap(), None);
            svc.clone().ready().await.unwrap().call(ctx).await.unwrap();
        }
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert_eq!(inner.calls(), 5);
    }
}
