/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;
use std::fmt::Debug;

type BoxError = Box<dyn Error + Send + Sync>;

/// Body type when a response is returned. Currently, the only use case is introspecting errors
/// so it is simply `Debug`. This is an area of potential design iteration.
// pub type Body = Pin<Box<dyn http_body::Body<Data = Bytes, Error=Box<dyn Error>> + Send + Sync>>;

/// Successful Sdk Result
///
/// Typically, transport implementations will type alias (or entirely wrap / transform) this type
/// plugging in a concrete body implementation, eg:
/// ```rust
/// # mod hyper {
/// #    pub struct Body;
/// # }
/// type SdkSuccess<O> = smithy_http::result::SdkSuccess<O, hyper::Body>;
/// ```
#[derive(Debug)]
pub struct SdkSuccess<O, B> {
    pub raw: http::Response<B>,
    pub parsed: O,
}

/// Failing Sdk Result
///
/// Typically, transport implementations will type alias (or entirely wrap / transform) this type
/// by specifying a concrete body implementation:
/// ```rust
/// # mod hyper {
/// #    pub struct Body;
/// # }
/// type SdkError<E> = smithy_http::result::SdkError<E, hyper::Body>;
/// ```
#[derive(Debug)]
pub enum SdkError<E, B> {
    /// The request failed during construction. It was not dispatched over the network.
    ConstructionFailure(BoxError),

    /// The request failed during dispatch. An HTTP response was not received. The request MAY
    /// have been sent.
    DispatchFailure(BoxError),

    /// A response was received but it was not parseable according the the protocol (for example
    /// the server hung up while the body was being read)
    ResponseError {
        raw: http::Response<B>,
        err: BoxError,
    },

    /// An error response was received from the service
    ServiceError {
        raw: http::Response<B>,
        err: E,
    },
}
