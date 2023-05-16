/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::BoxError;
use crate::client::interceptors::context::phase::Phase;
use crate::client::interceptors::InterceptorError;
use crate::client::orchestrator::HttpResponse;
use aws_smithy_http::result::{ConnectorError, SdkError};
use std::fmt;

#[non_exhaustive]
pub enum OrchestratorError<E> {
    /// An error occurred within an interceptor.
    Interceptor { err: InterceptorError },
    /// An error returned by a service.
    Operation { err: E },
    /// A general orchestrator error.
    Other { err: BoxError },
}

// TODO can I just assume that operation errors are all `Debug`?
impl<E> fmt::Debug for OrchestratorError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrchestratorError::Interceptor { err } => {
                write!(f, "{:?}", err)
            }
            OrchestratorError::Operation { .. } => {
                write!(f, "Operation Error")
            }
            OrchestratorError::Other { err } => {
                write!(f, "{:?}", err)
            }
        }
    }
}

impl<E> OrchestratorError<E> {
    /// Create a new `OrchestratorError` from a [`BoxError`].
    pub fn other(err: BoxError) -> Self {
        Self::Other { err }
    }

    /// Create a new `OrchestratorError` from an error received from a service.
    pub fn operation(err: E) -> Self {
        Self::Operation { err }
    }

    /// Create a new `OrchestratorError` from an [`InterceptorError`].
    pub fn interceptor(err: InterceptorError) -> Self {
        Self::Interceptor { err }
    }

    /// Convert the `OrchestratorError` into an [`SdkError`].
    pub fn into_sdk_error(
        self,
        phase: &Phase,
        response: Option<HttpResponse>,
    ) -> SdkError<E, HttpResponse> {
        match self {
            Self::Interceptor { err } => {
                use Phase::*;
                match phase {
                    BeforeSerialization | Serialization => SdkError::construction_failure(err),
                    BeforeTransmit | Transmit => match response {
                        Some(response) => SdkError::response_error(err, response),
                        None => SdkError::dispatch_failure(ConnectorError::other(err.into(), None)),
                    },
                    BeforeDeserialization | Deserialization | AfterDeserialization => {
                        SdkError::response_error(err, response.expect("phase has a response"))
                    }
                }
            }
            Self::Operation { err } => {
                debug_assert!(phase.is_after_deserialization(), "operation errors are a result of successfully receiving and parsing a response from the server. Therefore, we must be in the 'After Deserialization' phase.");
                SdkError::service_error(err, response.expect("phase has a response"))
            }
            Self::Other { err } => {
                use Phase::*;
                match phase {
                    BeforeSerialization | Serialization => SdkError::construction_failure(err),
                    BeforeTransmit | Transmit => convert_dispatch_error(err, response),
                    BeforeDeserialization | Deserialization | AfterDeserialization => {
                        SdkError::response_error(err, response.expect("phase has a response"))
                    }
                }
            }
        }
    }
}

fn convert_dispatch_error<O>(
    err: BoxError,
    response: Option<HttpResponse>,
) -> SdkError<O, HttpResponse> {
    let err = match err.downcast::<ConnectorError>() {
        Ok(connector_error) => {
            return SdkError::dispatch_failure(*connector_error);
        }
        Err(e) => e,
    };
    match response {
        Some(response) => SdkError::response_error(err, response),
        None => SdkError::dispatch_failure(ConnectorError::other(err.into(), None)),
    }
}

impl<O> From<InterceptorError> for OrchestratorError<O> {
    fn from(err: InterceptorError) -> Self {
        Self::interceptor(err)
    }
}

impl<O> From<BoxError> for OrchestratorError<O> {
    fn from(err: BoxError) -> Self {
        Self::other(err)
    }
}
