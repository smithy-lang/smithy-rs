/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![deny(missing_docs, missing_debug_implementations)]

//! Provides [`InstrumentOperation`] and a variety of helpers structures for dealing with sensitive
//! data.
//!
//! # Example
//!
//! ```
//! # use std::convert::Infallible;
//! # use aws_smithy_http_server::logging::*;
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
//! let sensitivity = Sensitivity::new()
//!     .request_header(|name| HeaderMarker {
//!        value: name == "header-name-a",
//!        key_suffix: None,
//!     })
//!     .path(|index| index % 2 == 0)
//!     .query(|name| QueryMarker { key: false, value: name == "bar" })
//!     .response_header(|name| {
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
//! let mut svc = InstrumentOperation::new(svc, "foo-operation").sensitivity(sensitivity);
//!
//! let _ = svc.call(request).await.unwrap();
//! # }
//! ```

mod headers;
mod sensitive;
mod service;
mod uri;

use std::fmt::{Debug, Display, Formatter};

pub use headers::*;
pub use sensitive::*;
pub use service::*;
pub use uri::*;

enum OrFmt<Left, Right> {
    Left(Left),
    Right(Right),
}

impl<Left, Right> Debug for OrFmt<Left, Right>
where
    Left: Debug,
    Right: Debug,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left(left) => left.fmt(f),
            Self::Right(right) => right.fmt(f),
        }
    }
}

impl<Left, Right> Display for OrFmt<Left, Right>
where
    Left: Display,
    Right: Display,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left(left) => left.fmt(f),
            Self::Right(right) => right.fmt(f),
        }
    }
}

/// The string placeholder for redacted data.
pub const REDACTED: &str = "{redacted}";
