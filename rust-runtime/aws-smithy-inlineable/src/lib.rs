/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Collection of modules that get conditionally included directly into the code generated
//! service crates.
//!
//! This is _NOT_ intended to be an actual crate. It is a cargo project to solely to aid
//! with local development of the SDK.

#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

/// Stub credentials provider for use when no credentials provider is used.
pub mod customizable_operation;

/// A module of client fakery that enables us to test customizable operations
pub(crate) mod client {
    use aws_smithy_client::{bounds, erase, retry};
    use aws_smithy_http::operation::Operation;
    use aws_smithy_http::result::{SdkError, SdkSuccess};
    use std::marker::PhantomData;

    use tower_service::Service;

    /// A fake Handle to enable testing of customizable operations
    #[derive(Debug)]
    pub(crate) struct Handle {
        pub client: Client,
    }

    /// A fake smithy client to enable testing of customizable operations
    #[derive(Debug)]
    pub(crate) struct Client<
        Connector = erase::DynConnector,
        Middleware = erase::DynMiddleware<Connector>,
        RetryPolicy = retry::Standard,
    > {
        _c: PhantomData<Connector>,
        _m: PhantomData<Middleware>,
        _r: PhantomData<RetryPolicy>,
    }

    impl<C, M, R> Client<C, M, R>
    where
        C: bounds::SmithyConnector,
        M: bounds::SmithyMiddleware<C>,
        R: retry::NewRequestPolicy,
    {
        pub(crate) async fn call<O, T, E, Retry>(
            &self,
            _input: Operation<O, Retry>,
        ) -> Result<T, SdkError<E>>
        where
            O: Send + Sync,
            Retry: Send + Sync,
            R::Policy: bounds::SmithyRetryPolicy<O, T, E, Retry>,
            bounds::Parsed<<M as bounds::SmithyMiddleware<C>>::Service, O, Retry>:
                Service<Operation<O, Retry>, Response = SdkSuccess<T>, Error = SdkError<E>> + Clone,
        {
            todo!()
        }
    }
}
