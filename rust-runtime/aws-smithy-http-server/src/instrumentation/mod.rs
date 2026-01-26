/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![deny(missing_docs, missing_debug_implementations)]

//! Provides [`InstrumentOperation`] and a variety of helpers structures for dealing with sensitive data. Together they
//! allow compliance with the [sensitive trait].
//!
//! # Example
//!
//! ```
//! # use std::convert::Infallible;
//! # use aws_smithy_http_server::instrumentation::{*, sensitivity::{*, headers::*, uri::*}};
//! # use aws_smithy_http_server::shape_id::ShapeId;
//! # use http::{Request, Response};
//! # use tower::{util::service_fn, Service};
//! # async fn service(request: Request<()>) -> Result<Response<()>, Infallible> {
//! #   Ok(Response::new(()))
//! # }
//! # async fn example() {
//! # let service = service_fn(service);
//! # const ID: ShapeId = ShapeId::new("namespace#foo-operation", "namespace", "foo-operation");
//! let request = Request::get("http://localhost/a/b/c/d?bar=hidden")
//!     .header("header-name-a", "hidden")
//!     .body(())
//!     .unwrap();
//!
//! let request_fmt = RequestFmt::new()
//!     .header(|name| HeaderMarker {
//!        value: name == "header-name-a",
//!        key_suffix: None,
//!     })
//!     .query(|name| QueryMarker { key: false, value: name == "bar" })
//!     .label(|index| index % 2 == 0, None);
//! let response_fmt = ResponseFmt::new()
//!     .header(|name| {
//!         if name.as_str().starts_with("prefix-") {
//!             HeaderMarker {
//!                 value: true,
//!                 key_suffix: Some("prefix-".len()),
//!             }
//!         } else {
//!             HeaderMarker {
//!                 value: name == "header-name-b",
//!                 key_suffix: None,
//!             }
//!         }
//!     })
//!     .status_code();
//! let mut service = InstrumentOperation::new(service, ID)
//!     .request_fmt(request_fmt)
//!     .response_fmt(response_fmt);
//!
//! let _ = service.call(request).await.unwrap();
//! # }
//! ```
//!
//! [sensitive trait]: https://smithy.io/2.0/spec/documentation-traits.html#sensitive-trait

mod plugin;
pub mod sensitivity;
mod service;

use std::fmt::{Debug, Display};

pub use plugin::*;
pub use service::*;

/// A standard interface for taking some component of the HTTP request/response and transforming it into new struct
/// which supports [`Debug`] or [`Display`]. This allows for polymorphism over formatting approaches.
pub trait MakeFmt<T> {
    /// Target of the `fmt` transformation.
    type Target;

    /// Transforms a source into a target, altering it's [`Display`] or [`Debug`] implementation.
    fn make(&self, source: T) -> Self::Target;
}

impl<T, U> MakeFmt<T> for &U
where
    U: MakeFmt<T>,
{
    type Target = U::Target;

    fn make(&self, source: T) -> Self::Target {
        U::make(self, source)
    }
}

/// Identical to [`MakeFmt`] but with a [`Display`] bound on the associated type.
pub trait MakeDisplay<T> {
    /// Mirrors [`MakeFmt::Target`].
    type Target: Display;

    /// Mirrors [`MakeFmt::make`].
    fn make_display(&self, source: T) -> Self::Target;
}

impl<T, U> MakeDisplay<T> for U
where
    U: MakeFmt<T>,
    U::Target: Display,
{
    type Target = U::Target;

    fn make_display(&self, source: T) -> Self::Target {
        U::make(self, source)
    }
}

/// Identical to [`MakeFmt`] but with a [`Debug`] bound on the associated type.
pub trait MakeDebug<T> {
    /// Mirrors [`MakeFmt::Target`].
    type Target: Debug;

    /// Mirrors [`MakeFmt::make`].
    fn make_debug(&self, source: T) -> Self::Target;
}

impl<T, U> MakeDebug<T> for U
where
    U: MakeFmt<T>,
    U::Target: Debug,
{
    type Target = U::Target;

    fn make_debug(&self, source: T) -> Self::Target {
        U::make(self, source)
    }
}

/// A blanket, identity, [`MakeFmt`] implementation. Applies no changes to the [`Display`]/[`Debug`] implementation.
#[derive(Debug, Clone, Default)]
pub struct MakeIdentity;

impl<T> MakeFmt<T> for MakeIdentity {
    type Target = T;

    fn make(&self, source: T) -> Self::Target {
        source
    }
}
