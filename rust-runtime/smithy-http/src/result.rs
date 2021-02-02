/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;
use std::fmt::Debug;

type BoxError = Box<dyn Error + Send + Sync>;

/// Body type when a response is returned. Currently, the only use case is introspecting errors
/// so it is simply `Debug`. This is an area of potential design iteration.
pub type Body = Box<dyn Debug>;

#[derive(Debug)]
pub struct SdkSuccess<O> {
    pub raw: http::Response<Box<dyn Debug>>,
    pub parsed: O,
}

#[derive(Debug)]
pub enum SdkError<E> {
    /// The request failed during construction. It was not dispatched over the network.
    ConstructionFailure(BoxError),

    /// The request failed during dispatch. An HTTP response was not received. The request MAY
    /// have been sent.
    DispatchFailure(BoxError),

    /// A response was received but it was not parseable according the the protocol (for example
    /// the server hung up while the body was being read)
    ResponseError {
        raw: http::Response<Box<dyn Debug>>,
        err: BoxError,
    },

    /// An error response was received from the service
    ServiceError {
        raw: http::Response<Box<dyn Debug>>,
        err: E,
    },
}
