/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// TODO: Remove this for final version
#![allow(unused)]

use crate::{Credentials, CredentialsError, ProvideCredentials};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, SystemTime};
use tracing::{error, info, trace_span};

// TODO: Make these configurable with a builder and sane defaults
const EXECUTOR_PANIC_BACKOFF: Duration = Duration::from_secs(10);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(5);
const REFRESH_BUFFER_TIME: Duration = Duration::from_secs(30);
const MAX_WAIT_TIME: Duration = Duration::from_millis(1000);

const WATCHDOG_STACK_SIZE_BYTES: usize = 256;

pub type AsyncCredentialResult = Result<Credentials, CredentialsError>;

// TODO: Add doc comment
pub trait AsyncCredentialLoader: Send + Sync {
    fn load_credentials(&self) -> Pin<Box<dyn Future<Output = AsyncCredentialResult> + Send>>;
}

mod error {
    use std::error::Error as StdError;
    use std::fmt;

    #[derive(Debug)]
    #[non_exhaustive]
    pub enum Error {
        RefreshFailed(Box<dyn StdError + Send + Sync>),
        ThreadSpawnFailed(Box<dyn StdError + Send + Sync>),
        ExecutorThreadFailed,
    }

    impl StdError for Error {}

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::RefreshFailed(err) => {
                    write!(f, "credential refresh failed: {}", err)
                }
                Error::ThreadSpawnFailed(err) => {
                    write!(f, "failed to spawn thread: {}", err)
                }
                Error::ExecutorThreadFailed => write!(f, "executor thread panicked"),
            }
        }
    }
}

pub use error::Error;

// TODO: Consider using https://docs.rs/arc-swap/1.3.0/arc_swap/
/// Stores a value in a RwLock and never holds the lock open across a section
/// of code that can panic in order to eliminate the PoisonedError.
#[derive(Clone)]
struct RwCell<T: Clone> {
    value: Arc<RwLock<T>>,
}

impl<T: Clone> RwCell<T> {
    pub fn new(initial: T) -> RwCell<T> {
        RwCell {
            value: Arc::new(RwLock::new(initial)),
        }
    }

    pub fn get(&self) -> T {
        self.value.read().expect("cannot be poisoned").clone()
    }

    pub fn set(&self, value: T) {
        *self.value.write().expect("cannot be poisoned") = value;
    }
}

#[derive(Clone)]
struct Inner {
    loader: Arc<dyn AsyncCredentialLoader>,
    credentials: RwCell<Option<Credentials>>,
}

impl Inner {
    pub async fn refresh(&self) -> Result<(), CredentialsError> {
        let credentials = self.loader.load_credentials().await?;
        self.credentials.set(Some(credentials));
        Ok(())
    }

    fn calc_next_refresh_sleep_time(now: SystemTime, expiration: SystemTime) -> Option<Duration> {
        let expiration = expiration - REFRESH_BUFFER_TIME;
        if now >= expiration {
            None
        } else {
            let remaining = expiration.duration_since(now).expect("now < expiration");
            if remaining > MAX_WAIT_TIME {
                Some(MAX_WAIT_TIME)
            } else {
                Some(remaining)
            }
        }
    }

    fn next_refresh_sleep_time(&self, now: SystemTime) -> Option<Duration> {
        let expiry = self.expiry();
        Self::calc_next_refresh_sleep_time(now, expiry)
    }

    fn expiry(&self) -> SystemTime {
        // TODO: Discuss requirements around refresh. Options:
        // 1. Default to some time if no expiration given by credentials (15 minutes? configurable?)
        // 2. Expect implementers of AsyncCredentialLoader to set the expiration if it's
        //    not set by its origin (such as STS).
        // 3. Support both #1 and #2 and make it configurable which is used?
        self.maybe_expiry()
            .expect("credential refresh loop can only be used with expiring credentials")
    }

    fn maybe_expiry(&self) -> Option<SystemTime> {
        self.credentials.get().map(|c| c.expiry()).flatten()
    }
}

#[derive(Copy, Clone)]
enum RefreshLoopState {
    Running,
    Canceled,
}

// TODO: Add doc comment
pub struct CancelableRefreshLoop {
    signal: Option<RwCell<RefreshLoopState>>,
}

impl CancelableRefreshLoop {
    pub fn cancel(self) {
        if let Some(signal) = self.signal {
            signal.set(RefreshLoopState::Canceled);
        }
    }
}

// TODO: Add doc comment
// TODO: Refactor to have the bulk of the logic be async runtime agnostic
// TODO: Refactor to facilitate testing
pub struct RefreshingCredentialProvider {
    inner: Inner,
    cancel_signal: Option<RwCell<RefreshLoopState>>,
}

impl RefreshingCredentialProvider {
    pub fn new(loader: Arc<dyn AsyncCredentialLoader>) -> RefreshingCredentialProvider {
        RefreshingCredentialProvider {
            inner: Inner {
                loader,
                credentials: RwCell::new(None),
            },
            cancel_signal: None,
        }
    }

    #[cfg(feature = "tokio-credentials")]
    async fn initial_refresh(&self) -> Result<(), Error> {
        let inner = self.inner.clone();
        let result = inner.loader.load_credentials().await;
        match result {
            Ok(credentials) => self.inner.credentials.set(Some(credentials)),
            Err(err) => return Err(Error::RefreshFailed(Box::new(err))),
        }
        Ok(())
    }

    #[cfg(feature = "tokio-credentials")]
    pub async fn spawn_refresh_loop(&mut self) -> Result<CancelableRefreshLoop, Error> {
        self.initial_refresh().await?;

        // Establish cancellation communication
        let cancel_signal = RwCell::new(RefreshLoopState::Running);
        self.cancel_signal = Some(cancel_signal.clone());

        // Start watchdog thread
        let inner = self.inner.clone();
        let watchdog_signal = cancel_signal.clone();
        thread::Builder::new()
            .stack_size(WATCHDOG_STACK_SIZE_BYTES)
            .name("RefreshingCredentialProvider-watchdog".into())
            .spawn(move || {
                Self::watchdog_thread(watchdog_signal, inner);
            })
            .map_err(|err| Error::ThreadSpawnFailed(Box::new(err)))?;

        Ok(CancelableRefreshLoop {
            signal: Some(cancel_signal),
        })
    }

    #[cfg(feature = "tokio-credentials")]
    fn watchdog_thread(signal: RwCell<RefreshLoopState>, inner: Inner) {
        while let RefreshLoopState::Running = signal.get() {
            // Spawn an executor thread to run user-provided async credential refresher.
            // This needs to have its own thread so that a panic won't end the refresh loop.
            let executor_signal = signal.clone();
            let executor_inner = inner.clone();
            let result = thread::Builder::new()
                .name("RefreshingCredentialProvider-executor".into())
                .spawn(move || {
                    RefreshingCredentialProvider::executor_thread(executor_signal, executor_inner);
                })
                .map_err(|err| Error::ThreadSpawnFailed(Box::new(err)))
                .and_then(|joiner| joiner.join().map_err(|_| Error::ExecutorThreadFailed));
            if let Err(err) = result {
                error!("RefreshingCredentialProvider executor failed: {}", err);
                thread::sleep(EXECUTOR_PANIC_BACKOFF);
            }
        }
    }

    #[cfg(feature = "tokio-credentials")]
    fn executor_thread(signal: RwCell<RefreshLoopState>, inner: Inner) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to start tokio runtime");
        runtime.block_on(async move {
            // TODO: Take a name argument from users to indicate which credentials are being refreshed?
            let span = trace_span!("credential_refresh_loop");
            let _enter = span.enter();

            info!("Starting credential refresh loop");
            Self::refresh_loop(signal, inner)
                .await
                .expect("refresh loop failed");
        });
    }

    #[cfg(feature = "tokio-credentials")]
    async fn refresh_loop(signal: RwCell<RefreshLoopState>, inner: Inner) -> Result<(), Error> {
        while let RefreshLoopState::Running = signal.get() {
            while let Some(sleep_duration) = inner.next_refresh_sleep_time(SystemTime::now()) {
                tokio::time::sleep(sleep_duration).await;
            }

            info!("Refreshing credentials");
            let timeout_future = tokio::time::sleep(REFRESH_TIMEOUT);
            let refresh_future = inner.refresh();
            tokio::pin!(timeout_future, refresh_future);
            loop {
                // In the event of timeout or failure, the sleep in the next iteration will be
                // skipped because the credential expiration time won't have changed.
                // TODO: Should we have backoff for both timeout and refresh failures, or just refresh failures?
                tokio::select! {
                    _ = timeout_future => {
                        error!("Credential refresh timed out after {} seconds", REFRESH_TIMEOUT.as_secs());
                        break;
                    }
                    result = refresh_future => {
                        if let Err(err) = result {
                            error!("Failed to refresh credentials: {}", err);
                        }
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl ProvideCredentials for RefreshingCredentialProvider {
    fn provide_credentials(&self) -> Result<Credentials, CredentialsError> {
        match self.inner.credentials.get() {
            Some(credentials) => Ok(credentials),
            None => Err(CredentialsError::CredentialsNotLoaded),
        }
    }
}

impl Drop for RefreshingCredentialProvider {
    fn drop(&mut self) {
        if let Some(signal) = &self.cancel_signal {
            signal.set(RefreshLoopState::Canceled);
        }
    }
}
