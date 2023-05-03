/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::error::Error;
use std::error::Error as StdError;
use std::future::Future;
use std::time::Duration;
use tracing::{error, info};

pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

pub enum ErrorClass {
    Retry,
    NoRetry,
}

#[derive(thiserror::Error, Debug)]
pub enum RetryError {
    #[error("failed with unretryable error")]
    FailedUnretryable(#[source] Box<dyn Error + Send + Sync + 'static>),
    #[error("failed {0} times and won't be retried again")]
    FailedMaxAttempts(usize),
}

pub async fn run_with_retry<F, Ft, C, O, E>(
    what: &str,
    max_attempts: usize,
    backoff: Duration,
    create_future: F,
    classify_error: C,
) -> Result<O, RetryError>
where
    F: Fn() -> Ft,
    Ft: Future<Output = Result<O, E>> + Send,
    C: Fn(&E) -> ErrorClass,
    E: Into<BoxError>,
{
    assert!(max_attempts > 0);

    let mut attempt = 1;
    loop {
        let future = create_future();
        match future.await {
            Ok(output) => return Ok(output),
            Err(err) => {
                match classify_error(&err) {
                    ErrorClass::NoRetry => {
                        return Err(RetryError::FailedUnretryable(err.into()));
                    }
                    ErrorClass::Retry => {
                        info!(
                            "{} failed on attempt {} with retryable error: {:?}. Will retry after {:?}",
                            what, attempt, err.into(), backoff
                        );
                    }
                }
            }
        }

        // If we made it this far, we're retrying or failing at max retries
        if attempt == max_attempts {
            return Err(RetryError::FailedMaxAttempts(max_attempts));
        }
        attempt += 1;
        tokio::time::sleep(backoff).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    #[derive(thiserror::Error, Debug)]
    #[error("FakeError")]
    struct FakeError;

    #[derive(thiserror::Error, Debug)]
    #[error("UnretryableError")]
    struct UnretryableError;

    fn assert_send<T: Send>(thing: T) -> T {
        thing
    }

    #[tokio::test]
    async fn fail_max_attempts() {
        let attempt = Arc::new(AtomicU8::new(1));
        let result = {
            let attempt = attempt.clone();
            assert_send(run_with_retry(
                "test",
                3,
                Duration::from_millis(0),
                move || {
                    let attempt = attempt.clone();
                    Box::pin(async move {
                        attempt.fetch_add(1, Ordering::Relaxed);
                        Result::<(), _>::Err(FakeError)
                    })
                },
                |_err| ErrorClass::Retry,
            ))
            .await
        };

        assert!(matches!(result, Err(RetryError::FailedMaxAttempts(3))));
        // `attempt` holds the number of the next attempt, so 4 instead of 3 in this case
        assert_eq!(4, attempt.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn fail_then_succeed() {
        let attempt = Arc::new(AtomicU8::new(1));
        let result = {
            let attempt = attempt.clone();
            run_with_retry(
                "test",
                3,
                Duration::from_millis(0),
                move || {
                    let attempt = attempt.clone();
                    Box::pin(async move {
                        if attempt.fetch_add(1, Ordering::Relaxed) == 1 {
                            Err(FakeError)
                        } else {
                            Ok(2)
                        }
                    })
                },
                |_err| ErrorClass::Retry,
            )
            .await
        };

        assert!(matches!(result, Ok(2)));
        // `attempt` holds the number of the next attempt, so 3 instead of 2 in this case
        assert_eq!(3, attempt.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn unretryable_error() {
        let attempt = Arc::new(AtomicU8::new(1));
        let result = {
            let attempt = attempt.clone();
            run_with_retry(
                "test",
                3,
                Duration::from_millis(0),
                move || {
                    let attempt = attempt.clone();
                    Box::pin(async move {
                        if attempt.fetch_add(1, Ordering::Relaxed) == 1 {
                            Err(UnretryableError)
                        } else {
                            Ok(2)
                        }
                    })
                },
                |err| {
                    if matches!(err, UnretryableError) {
                        ErrorClass::NoRetry
                    } else {
                        ErrorClass::Retry
                    }
                },
            )
            .await
        };

        match result {
            Err(RetryError::FailedUnretryable(err)) => {
                assert!(err.downcast_ref::<UnretryableError>().is_some());
            }
            _ => panic!("should be an unretryable error"),
        }
        assert_eq!(2, attempt.load(Ordering::Relaxed));
    }
}
