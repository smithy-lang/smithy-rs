/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Macros implementation

// This file is a copy of Axum rejection.rs (https://github.com/tokio-rs/axum/blob/2507463706d0cea90007b5959c579a32d4b24cc4/axum/src/extract/rejection.rs#L1)
// with simple modification to allow public visibility and re-use of the generated structures.
// Axum original license can be found inside the file named LICENSE.mit.

// Define a single rejection type
macro_rules! define_rejection {
    (
        #[status = $status:ident]
        #[body = $body:expr]
        $(#[$m:meta])*
        pub struct $name:ident;
    ) => {
        $(#[$m])*
        #[derive(Debug)]
        pub struct $name;

        #[allow(deprecated)]
        impl axum::response::IntoResponse for $name {
            type Body = http_body::Full<bytes::Bytes>;
            type BodyError = std::convert::Infallible;

            fn into_response(self) -> http::Response<Self::Body> {
                let mut res = http::Response::new(http_body::Full::from($body));
                *res.status_mut() = http::StatusCode::$status;
                res
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", $body)
            }
        }

        impl std::error::Error for $name {}
    };

    (
        #[status = $status:ident]
        #[body = $body:expr]
        $(#[$m:meta])*
        pub struct $name:ident (Error);
    ) => {
        $(#[$m])*
        #[derive(Debug)]
        pub struct $name(crate::Error);

        impl $name {
            pub fn from_err<E>(err: E) -> Self
            where
                E: Into<crate::BoxError>,
            {
                Self(crate::Error::new(err))
            }
        }

        impl IntoResponse for $name {
            type Body = http_body::Full<bytes::Bytes>;
            type BodyError = std::convert::Infallible;

            fn into_response(self) -> http::Response<Self::Body> {
                let mut res =
                    http::Response::new(http_body::Full::from(format!(concat!($body, ": {}"), self.0)));
                *res.status_mut() = http::StatusCode::$status;
                res
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", $body)
            }
        }

        impl std::error::Error for $name {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
    };
}

// Define a composite rejection type
macro_rules! composite_rejection {
    (
        $(#[$m:meta])*
        pub enum $name:ident {
            $($variant:ident),+
            $(,)?
        }
    ) => {
        $(#[$m])*
        #[derive(Debug)]
        pub enum $name {
            $(
                #[allow(missing_docs, deprecated)]
                $variant($variant)
            ),+
        }

        impl axum::response::IntoResponse for $name {
            type Body = http_body::Full<bytes::Bytes>;
            type BodyError = std::convert::Infallible;

            fn into_response(self) -> http::Response<Self::Body> {
                match self {
                    $(
                        Self::$variant(inner) => inner.into_response(),
                    )+
                }
            }
        }

        $(
            #[allow(deprecated)]
            impl From<$variant> for $name {
                fn from(inner: $variant) -> Self {
                    Self::$variant(inner)
                }
            }
        )+

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(
                        Self::$variant(inner) => write!(f, "{}", inner),
                    )+
                }
            }
        }

        impl std::error::Error for $name {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    $(
                        Self::$variant(inner) => Some(inner),
                    )+
                }
            }
        }
    };
}
