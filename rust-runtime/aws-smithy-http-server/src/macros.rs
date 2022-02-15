/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code was copied and then modified from Tokio's Axum.

/* Copyright (c) 2021 Tower Contributors
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software
 * is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 */

//! Macros implementation.

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

        impl axum_core::response::IntoResponse for $name {
            fn into_response(self) -> axum_core::response::Response {
                let mut res = http::Response::new(axum_core::body::boxed(http_body::Full::from($body)));
                *res.status_mut() = http::StatusCode::$status;
                res.extensions_mut().insert(
                    crate::ExtensionRejection::new(self.to_string())
                );
                res
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", $body)
            }
        }

        impl std::error::Error for $name {}

        impl Default for $name {
            fn default() -> Self {
                Self
            }
        }
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
                E: Into<$crate::BoxError>,
            {
                Self(crate::Error::new(err))
            }
        }

        impl axum_core::response::IntoResponse for $name {

            fn into_response(self) -> axum_core::response::Response {
                let body = http_body::Full::from(format!(concat!($body, ": {}"), self.0));
                let body = $crate::body::boxed(body);
                let mut res =
                    http::Response::new(body);
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

        impl From<&str> for $name {
            fn from(src: &str) -> Self {
                Self(Err(src).expect("Unable to convert string into error"))
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

        impl axum_core::response::IntoResponse for $name {

            fn into_response(self) -> axum_core::response::Response {
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
    }
}

/// Define a type that implements [`std::future::Future`].
#[macro_export]
macro_rules! opaque_future {
    ($(#[$m:meta])* pub type $name:ident = $actual:ty;) => {
        opaque_future! {
            $(#[$m])*
            #[allow(clippy::type_complexity)]
            pub type $name<> = $actual;
        }
    };

    ($(#[$m:meta])* pub type $name:ident<$($param:ident),*> = $actual:ty;) => {
            pin_project_lite::pin_project! {
                $(#[$m])*
                pub struct $name<$($param),*> {
                    #[pin] future: $actual,
                }
            }

        impl<$($param),*> $name<$($param),*> {
            pub(crate) fn new(future: $actual) -> Self {
                Self { future }
            }
        }

        impl<$($param),*> std::fmt::Debug for $name<$($param),*> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_tuple(stringify!($name)).field(&format_args!("...")).finish()
            }
        }

        impl<$($param),*> std::future::Future for $name<$($param),*>
        where
            $actual: std::future::Future,
        {
            type Output = <$actual as std::future::Future>::Output;

            #[inline]
            fn poll(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Self::Output> {
                self.project().future.poll(cx)
            }
        }
    };
}

pub use opaque_future;

/// Implements `Deref` for all `Extension` holding a `&'static, str`.
macro_rules! impl_deref {
    ($name:ident) => {
        impl Deref for $name {
            type Target = &'static str;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

/// Implements `new` for all `Extension` holding a `&'static, str`.
macro_rules! impl_extension_new_and_deref {
    ($name:ident) => {
        impl $name {
            #[doc = concat!("Returns a new `", stringify!($name), "`.")]
            pub fn new(value: &'static str) -> $name {
                $name(value)
            }
        }

        impl_deref!($name);
    };
}
