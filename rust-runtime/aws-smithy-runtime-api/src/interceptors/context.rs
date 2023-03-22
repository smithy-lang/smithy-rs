/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::InterceptorError;
use crate::type_erasure::TypeErasedBox;

pub type Input = TypeErasedBox;
pub type Output = TypeErasedBox;
pub type Error = TypeErasedBox;
pub type OutputOrError = Result<Output, Error>;

/// A container for the data currently available to an interceptor.
pub struct InterceptorContext<TxReq, TxRes> {
    input: Input,
    output_or_error: Option<OutputOrError>,
    tx_request: Option<TxReq>,
    tx_response: Option<TxRes>,
}

// TODO(interceptors) we could use types to ensure that people calling methods on interceptor context can't access
//     field that haven't been set yet.
impl<TxReq, TxRes> InterceptorContext<TxReq, TxRes> {
    pub fn new(input: Input) -> Self {
        Self {
            input,
            output_or_error: None,
            tx_request: None,
            tx_response: None,
        }
    }

    /// Retrieve the modeled request for the operation being invoked.
    pub fn input(&self) -> &Input {
        &self.input
    }

    /// Retrieve the modeled request for the operation being invoked.
    pub fn input_mut(&mut self) -> &mut Input {
        &mut self.input
    }

    /// Retrieve the transmittable request for the operation being invoked.
    /// This will only be available once request marshalling has completed.
    pub fn tx_request(&self) -> Result<&TxReq, InterceptorError> {
        self.tx_request
            .as_ref()
            .ok_or_else(InterceptorError::invalid_tx_request_access)
    }

    /// Retrieve the transmittable request for the operation being invoked.
    /// This will only be available once request marshalling has completed.
    pub fn tx_request_mut(&mut self) -> Result<&mut TxReq, InterceptorError> {
        self.tx_request
            .as_mut()
            .ok_or_else(InterceptorError::invalid_tx_request_access)
    }

    /// Retrieve the response to the transmittable request for the operation
    /// being invoked. This will only be available once transmission has
    /// completed.
    pub fn tx_response(&self) -> Result<&TxRes, InterceptorError> {
        self.tx_response
            .as_ref()
            .ok_or_else(InterceptorError::invalid_tx_response_access)
    }

    /// Retrieve the response to the transmittable request for the operation
    /// being invoked. This will only be available once transmission has
    /// completed.
    pub fn tx_response_mut(&mut self) -> Result<&mut TxRes, InterceptorError> {
        self.tx_response
            .as_mut()
            .ok_or_else(InterceptorError::invalid_tx_response_access)
    }

    /// Retrieve the response to the customer. This will only be available
    /// once the `tx_response` has been unmarshalled or the
    /// attempt/execution has failed.
    pub fn output_or_error(&self) -> Result<Result<&Output, &Error>, InterceptorError> {
        self.output_or_error
            .as_ref()
            .ok_or_else(InterceptorError::invalid_modeled_response_access)
            .map(|res| res.as_ref())
    }

    /// Retrieve the response to the customer. This will only be available
    /// once the `tx_response` has been unmarshalled or the
    /// attempt/execution has failed.
    pub fn output_or_error_mut(&mut self) -> Result<&mut Result<Output, Error>, InterceptorError> {
        self.output_or_error
            .as_mut()
            .ok_or_else(InterceptorError::invalid_modeled_response_access)
    }

    // There is no set_modeled_request method because that can only be set once, during context construction

    pub fn set_tx_request(&mut self, transmit_request: TxReq) {
        if self.tx_request.is_some() {
            panic!("Called set_tx_request but a transmit_request was already set. This is a bug, pleases report it.");
        }

        self.tx_request = Some(transmit_request);
    }

    pub fn set_tx_response(&mut self, transmit_response: TxRes) {
        if self.tx_response.is_some() {
            panic!("Called set_tx_response but a transmit_response was already set. This is a bug, pleases report it.");
        }

        self.tx_response = Some(transmit_response);
    }

    pub fn set_output_or_error(&mut self, output: Result<Output, Error>) {
        if self.output_or_error.is_some() {
            panic!(
                "Called set_output but an output was already set. This is a bug. Please report it."
            );
        }

        self.output_or_error = Some(output);
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> (Input, Option<OutputOrError>, Option<TxReq>, Option<TxRes>) {
        (
            self.input,
            self.output_or_error,
            self.tx_request,
            self.tx_response,
        )
    }
}
