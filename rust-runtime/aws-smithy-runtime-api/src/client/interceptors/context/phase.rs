/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::interceptors::context::{Error, Output};
use crate::client::interceptors::{BoxError, InterceptorError};
use crate::client::orchestrator::HttpResponse;
use aws_smithy_http::result::{ConnectorError, SdkError};

pub(crate) fn convert_construction_failure(
    error: BoxError,
    _: Option<Result<Output, Error>>,
    _: Option<HttpResponse>,
) -> SdkError<Error, HttpResponse> {
    SdkError::construction_failure(error)
}

pub(crate) fn convert_dispatch_error(
    error: BoxError,
    _: Option<Result<Output, Error>>,
    response: Option<HttpResponse>,
) -> SdkError<Error, HttpResponse> {
    let error = match error.downcast::<ConnectorError>() {
        Ok(connector_error) => {
            return SdkError::dispatch_failure(*connector_error);
        }
        Err(e) => e,
    };
    if let Some(response) = response {
        SdkError::response_error(error, response)
    } else {
        SdkError::dispatch_failure(ConnectorError::other(error, None))
    }
}

pub(crate) fn convert_response_handling_error(
    error: BoxError,
    output_or_error: Option<Result<Output, Error>>,
    response: Option<HttpResponse>,
) -> SdkError<Error, HttpResponse> {
    match (response, output_or_error) {
        (Some(response), Some(Err(error))) => SdkError::service_error(error, response),
        (Some(response), _) => SdkError::response_error(error, response),
        _ => unreachable!("phase has a response"),
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Phase {
    /// Represents the phase of an operation prior to serialization.
    BeforeSerialization,
    /// Represents the phase of an operation where the request is serialized.
    Serialization,
    /// Represents the phase of an operation prior to transmitting a request over the network.
    BeforeTransmit,
    /// Represents the phase of an operation where the request is transmitted over the network.
    Transmit,
    /// Represents the phase of an operation prior to parsing a response.
    BeforeDeserialization,
    /// Represents the phase of an operation where the response is parsed.
    Deserialization,
    /// Represents the phase of an operation after parsing a response.
    AfterDeserialization,
}

impl Phase {
    pub fn is_before_serialization(&self) -> bool {
        matches!(self, Self::BeforeSerialization)
    }

    pub fn is_serialization(&self) -> bool {
        matches!(self, Self::Serialization)
    }

    pub fn is_before_transmit(&self) -> bool {
        matches!(self, Self::BeforeTransmit)
    }

    pub fn is_transmit(&self) -> bool {
        matches!(self, Self::Transmit)
    }

    pub fn is_before_deserialization(&self) -> bool {
        matches!(self, Self::BeforeDeserialization)
    }

    pub fn is_deserialization(&self) -> bool {
        matches!(self, Self::Deserialization)
    }

    pub fn fail_with_box_error(
        &self,
        error: BoxError,
        output_or_error: Option<Result<Output, Error>>,
        response: Option<HttpResponse>,
    ) -> SdkError<Error, HttpResponse> {
        use Phase::*;
        match self {
            BeforeSerialization | Serialization => {
                convert_construction_failure(error, output_or_error, response)
            }
            BeforeTransmit | Transmit => convert_dispatch_error(error, output_or_error, response),
            BeforeDeserialization | Deserialization | AfterDeserialization => {
                convert_response_handling_error(error, output_or_error, response)
            }
        }
    }

    // TODO yes, this is completely awful. I want to get this working and then I'll think about how to do it better.
    //    - Zelda
    pub fn fail_with_type_erased_error(
        &self,
        error: Error,
        response: Option<HttpResponse>,
    ) -> SdkError<Error, HttpResponse> {
        use Phase::*;
        let error = match error.downcast::<InterceptorError>() {
            Ok(interceptor_error) => match self {
                BeforeSerialization | Serialization => {
                    return SdkError::construction_failure(interceptor_error);
                }
                BeforeTransmit | Transmit => {
                    return if let Some(response) = response {
                        SdkError::response_error(interceptor_error, response)
                    } else {
                        SdkError::dispatch_failure(ConnectorError::other(interceptor_error, None))
                    }
                }
                BeforeDeserialization | Deserialization | AfterDeserialization => match response {
                    Some(response) => {
                        return SdkError::response_error(interceptor_error, response);
                    }
                    None => unreachable!("phase has a response"),
                },
            },
            Err(e) => e,
        };

        match self {
            BeforeSerialization | Serialization => SdkError::construction_failure(error),
            BeforeTransmit | Transmit => {
                let error = match error.downcast::<ConnectorError>() {
                    Ok(connector_error) => {
                        return SdkError::dispatch_failure(*connector_error);
                    }
                    Err(e) => e,
                };
                if let Some(response) = response {
                    SdkError::response_error(error, response)
                } else {
                    unreachable!("Dispatch has failed but the error was unexpected. Please file a bug report and let me know what this is: {:?}", error);
                }
            }
            BeforeDeserialization | Deserialization | AfterDeserialization => match response {
                Some(response) => SdkError::response_error(error, response),
                None => unreachable!("phase has a response"),
            },
        }
    }
}
