/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The combination of [HTTP binding traits] and the [sensitive trait] require us to redact
//! portions of the HTTP requests/responses during logging.
//!
//! [HTTP binding traits]: https://smithy.io/2.0/spec/http-bindings.html
//! [sensitive trait]: https://smithy.io/2.0/spec/documentation-traits.html#sensitive-trait

pub mod headers;
mod request;
mod response;
mod sensitive;
pub mod uri;

use crate::http::{HeaderMap, StatusCode, Uri};
pub use request::*;
pub use response::*;
pub use sensitive::*;

use super::{MakeDebug, MakeDisplay};

/// The string placeholder for redacted data.
pub const REDACTED: &str = "{redacted}";

/// An interface for providing [`MakeDebug`] and [`MakeDisplay`] for [`Request`](http::Request) and
/// [`Response`](http::Response).
pub trait Sensitivity {
    /// The [`MakeDebug`] and [`MakeDisplay`] for the request [`HeaderMap`] and [`Uri`].
    type RequestFmt: for<'a> MakeDebug<&'a HeaderMap> + for<'a> MakeDisplay<&'a Uri>;
    /// The [`MakeDebug`] and [`MakeDisplay`] for the response [`HeaderMap`] and [`StatusCode`].
    type ResponseFmt: for<'a> MakeDebug<&'a HeaderMap> + MakeDisplay<StatusCode>;

    /// Returns the [`RequestFmt`](Sensitivity::RequestFmt).
    fn request_fmt() -> Self::RequestFmt;

    /// Returns the [`ResponseFmt`](Sensitivity::ResponseFmt).
    fn response_fmt() -> Self::ResponseFmt;
}
