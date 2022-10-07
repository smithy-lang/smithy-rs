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
//! # use http::{Request, Response};
//! # use tower::{util::service_fn, Service};
//! # async fn service(request: Request<()>) -> Result<Response<()>, Infallible> {
//! #   Ok(Response::new(()))
//! # }
//! # async fn example() {
//! # let svc = service_fn(service);
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
//! let mut svc = InstrumentOperation::new(svc, "foo-operation")
//!     .request_fmt(request_fmt)
//!     .response_fmt(response_fmt);
//!
//! let _ = svc.call(request).await.unwrap();
//! # }
//! ```
//!
//! [sensitive trait]: https://awslabs.github.io/smithy/1.0/spec/core/documentation-traits.html?highlight=sensitive%20trait#sensitive-trait

mod layer;
mod plugin;
mod service;

pub use plugin::*;
pub use layer::*;
pub use service::*;
