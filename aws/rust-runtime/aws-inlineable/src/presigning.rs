/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// TODO(PresignedReqPrototype): Add doc comments

pub mod config {
    use std::fmt;
    use std::time::{Duration, SystemTime};

    #[non_exhaustive]
    #[derive(Debug, Clone)]
    pub struct PresigningConfig {
        start_time: SystemTime,
        expires_in: Duration,
    }

    impl PresigningConfig {
        pub fn expires_in(expires_in: Duration) -> Result<PresigningConfig, Error> {
            Self::builder().expires_in(expires_in).build()
        }

        pub fn builder() -> Builder {
            Builder::default()
        }
    }

    #[non_exhaustive]
    #[derive(Debug)]
    pub enum Error {
        ExpiresInDurationTooLong,
        ExpiresInRequired,
    }

    impl std::error::Error for Error {}

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::ExpiresInDurationTooLong => {
                    write!(f, "`expires_in` must be no longer than one week")
                }
                Error::ExpiresInRequired => write!(f, "`expires_in` is required"),
            }
        }
    }

    #[non_exhaustive]
    #[derive(Default, Debug)]
    pub struct Builder {
        start_time: Option<SystemTime>,
        expires_in: Option<Duration>,
    }

    impl Builder {
        pub fn start_time(mut self, start_time: SystemTime) -> Self {
            self.set_start_time(Some(start_time));
            self
        }
        pub fn set_start_time(&mut self, start_time: Option<SystemTime>) {
            self.start_time = start_time;
        }

        pub fn expires_in(mut self, expires_in: Duration) -> Self {
            self.set_expires_in(Some(expires_in));
            self
        }
        pub fn set_expires_in(&mut self, expires_in: Option<Duration>) {
            self.expires_in = expires_in;
        }

        pub fn build(self) -> Result<PresigningConfig, Error> {
            let expires_in = self.expires_in.ok_or(Error::ExpiresInRequired)?;
            if expires_in > Duration::from_secs(604800) {
                return Err(Error::ExpiresInDurationTooLong);
            }
            Ok(PresigningConfig {
                start_time: self.start_time.unwrap_or_else(SystemTime::now),
                expires_in,
            })
        }
    }
}

pub mod request {
    #[non_exhaustive]
    #[derive(Debug)]
    pub struct PresignedRequest(http::Request<()>);

    impl PresignedRequest {
        pub(crate) fn new(inner: http::Request<()>) -> Self {
            Self(inner)
        }

        pub fn method(&self) -> &http::Method {
            self.0.method()
        }

        pub fn uri(&self) -> &http::Uri {
            self.0.uri()
        }

        pub fn headers(&self) -> &http::HeaderMap<http::HeaderValue> {
            self.0.headers()
        }
    }
}
