/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Interceptor context.
//!
//! Interceptors have access to varying pieces of context during the course of an operation.
//!
//! An operation is composed of multiple phases. The initial phase is [`Phase::BeforeSerialization`], which
//! has the original input as context. The next phase is [`Phase::BeforeTransmit`], which has the serialized
//! request as context. Depending on which hook is being called with the dispatch context,
//! the serialized request may or may not be signed (which should be apparent from the hook name).
//! Following the [`Phase::BeforeTransmit`] phase is the [`Phase::BeforeDeserialization`] phase, which has
//! the raw response available as context. Finally, the [`Phase::AfterDeserialization`] phase
//! has both the raw and parsed response available.
//!
//! To summarize:
//! 1. [`Phase::BeforeSerialization`]: Only has the operation input.
//! 2. [`Phase::BeforeTransmit`]: Only has the serialized request.
//! 3. [`Phase::BeforeDeserialization`]: Has the raw response.
//! 3. [`Phase::AfterDeserialization`]: Has the raw response and the parsed response.
//!
//! When implementing hooks, if information from a previous phase is required, then implement
//! an earlier hook to examine that context, and save off any necessary information into the
//! [`ConfigBag`] for later hooks to examine.  Interior mutability is **NOT**
//! recommended for storing request-specific information in your interceptor implementation.
//! Use the [`ConfigBag`] instead.

/// Operation phases.
pub mod phase;
mod wrappers;

use crate::client::orchestrator::{HttpRequest, HttpResponse};
use crate::config_bag::ConfigBag;
use crate::type_erasure::{TypeErasedBox, TypeErasedError};
use aws_smithy_http::result::SdkError;
use phase::Phase;
use std::mem;
use tracing::{error, trace};

use crate::client::interceptors::context::phase::{
    convert_construction_failure, convert_dispatch_error, convert_response_handling_error,
};
use crate::client::interceptors::BoxError;
pub use wrappers::{
    AfterDeserializationInterceptorContextMut, AfterDeserializationInterceptorContextRef,
    BeforeDeserializationInterceptorContextMut, BeforeDeserializationInterceptorContextRef,
    BeforeSerializationInterceptorContextMut, BeforeSerializationInterceptorContextRef,
    BeforeTransmitInterceptorContextMut, BeforeTransmitInterceptorContextRef,
};

pub type Input = TypeErasedBox;
pub type Output = TypeErasedBox;
pub type Error = TypeErasedError;
pub type OutputOrError = Result<Output, Error>;

type Request = HttpRequest;
type Response = HttpResponse;

/// A container for the data currently available to an interceptor.
///
/// Different context is available based on which phase the operation is currently in. For example,
/// context in the [`Phase::BeforeSerialization`] phase won't have a `request` yet since the input hasn't been
/// serialized at that point. But once it gets into the [`Phase::BeforeTransmit`] phase, the `request` will be set.
#[derive(Debug)]
pub struct InterceptorContext<I = Input, O = Output, E = Error> {
    input: Option<I>,
    output_or_error: Option<Result<O, E>>,
    request: Option<Request>,
    response: Option<Response>,
    phase: Phase,
    tainted: bool,
    request_checkpoint: Option<HttpRequest>,
    is_failed: bool,
}

impl InterceptorContext<Input, Output, Error> {
    /// Creates a new interceptor context in the [`Phase::BeforeSerialization`] phase.
    pub fn new(input: Input) -> InterceptorContext<Input, Output, Error> {
        InterceptorContext {
            input: Some(input),
            output_or_error: None,
            request: None,
            response: None,
            phase: Phase::BeforeSerialization,
            tainted: false,
            request_checkpoint: None,
            is_failed: false,
        }
    }

    /// Mark this context as failed due to errors during the operation. Any errors already contained
    /// by the context will be replaced by the given error.
    pub fn fail(&mut self, error: TypeErasedError) {
        if !self.is_failed {
            trace!(
                "orchestrator is transitioning to the 'failure' phase from the '{:?}' phase",
                self.phase
            );
        }
        if let Some(Err(existing_err)) = mem::replace(&mut self.output_or_error, Some(Err(error))) {
            error!("orchestrator context received an error but one was already present; Throwing away previous error: {:?}", existing_err);
        }

        self.is_failed = true;
    }
}

impl<I, O, E> InterceptorContext<I, O, E> {
    /// Decomposes the context into its constituent parts.
    #[doc(hidden)]
    #[allow(clippy::type_complexity)]
    pub fn into_parts(
        self,
    ) -> (
        Option<I>,
        Option<Result<O, E>>,
        Option<Request>,
        Option<Response>,
    ) {
        (
            self.input,
            self.output_or_error,
            self.request,
            self.response,
        )
    }

    /// Retrieve the input for the operation being invoked.
    pub fn input(&self) -> &I {
        debug_assert!(
            self.phase.is_before_serialization(),
            "called input but phase is not 'before serialization'"
        );
        self.input
            .as_ref()
            .expect("input is present in 'before serialization'")
    }

    /// Retrieve the input for the operation being invoked.
    pub fn input_mut(&mut self) -> &mut I {
        debug_assert!(
            self.phase.is_before_serialization(),
            "called input but phase is not 'before serialization'"
        );
        self.input
            .as_mut()
            .expect("input is present in 'before serialization'")
    }

    /// Takes ownership of the input.
    pub fn take_input(&mut self) -> Option<I> {
        debug_assert!(
            self.phase.is_serialization(),
            "called take_input but phase is not 'serialization'"
        );
        self.input.take()
    }

    /// Set the request for the operation being invoked.
    pub fn set_request(&mut self, request: Request) {
        debug_assert!(
            self.phase.is_serialization(),
            "called set_request but phase is not 'serialization'"
        );
        debug_assert!(
            self.request.is_none(),
            "called set_request but a request was already set"
        );
        self.request = Some(request);
    }

    /// Retrieve the transmittable request for the operation being invoked.
    /// This will only be available once request marshalling has completed.
    pub fn request(&self) -> &Request {
        debug_assert!(
            self.phase.is_before_transmit(),
            "called request but phase is not 'before transmit'"
        );
        self.request
            .as_ref()
            .expect("request populated in 'before transmit'")
    }

    /// Retrieve the transmittable request for the operation being invoked.
    /// This will only be available once request marshalling has completed.
    pub fn request_mut(&mut self) -> &mut Request {
        debug_assert!(
            self.phase.is_before_transmit(),
            "called request_mut but phase is not 'before transmit'"
        );
        self.request
            .as_mut()
            .expect("request populated in 'before transmit'")
    }

    /// Takes ownership of the request.
    pub fn take_request(&mut self) -> Request {
        debug_assert!(
            self.phase.is_transmit(),
            "called take_request  but phase is not 'transmit'"
        );
        debug_assert!(self.request.is_some());
        self.request
            .take()
            .expect("take request once during 'transmit'")
    }

    /// Set the response for the operation being invoked.
    pub fn set_response(&mut self, response: Response) {
        debug_assert!(
            self.phase.is_transmit(),
            "called set_response but phase is not 'transmit'"
        );
        debug_assert!(
            self.response.is_none(),
            "called set_response but a response was already set"
        );
        self.response = Some(response);
    }

    /// Returns the response.
    pub fn response(&self) -> &Response {
        debug_assert!(
            self.phase.is_before_deserialization()
                || self.phase.is_deserialization()
                || self.phase.is_after_deserialization(),
            "called response but phase is not 'before deserialization'"
        );
        self.response.as_ref().expect(
            "response set in 'before deserialization' and available in the phases following it",
        )
    }

    /// Returns a mutable reference to the response.
    pub fn response_mut(&mut self) -> &mut Response {
        debug_assert!(
            self.phase.is_before_deserialization()
                || self.phase.is_deserialization()
                || self.phase.is_after_deserialization(),
            "called response_mut but phase is not 'before deserialization'"
        );
        self.response.as_mut().expect(
            "response is set in 'before deserialization' and available in the following phases",
        )
    }

    /// Set the output or error for the operation being invoked.
    pub fn set_output_or_error(&mut self, output: Result<O, E>) {
        debug_assert!(
            self.phase.is_deserialization(),
            "called set_output_or_error but phase is not 'deserialization'"
        );
        debug_assert!(
            self.output_or_error.is_none(),
            "called set_output_or_error but output_or_error was already set"
        );

        self.output_or_error = Some(output);
    }

    /// Returns the deserialized output or error.
    pub fn output_or_error(&self) -> Result<&O, &E> {
        debug_assert!(
            self.phase.is_after_deserialization(),
            "output_or_error was called but phase is not 'after deserialization'"
        );
        self.output_or_error
            .as_ref()
            .expect("output set in Phase::AfterDeserialization")
            .as_ref()
    }

    /// Returns the mutable reference to the deserialized output or error.
    pub fn output_or_error_mut(&mut self) -> &mut Result<O, E> {
        debug_assert!(
            self.phase.is_after_deserialization(),
            "output_or_error_mut is only available in the 'after deserialization' phase"
        );
        self.output_or_error
            .as_mut()
            .expect("output set in 'after deserialization'")
    }

    /// Advance to the Serialization phase.
    #[doc(hidden)]
    pub fn enter_serialization_phase(&mut self) {
        debug_assert!(
            self.phase.is_before_serialization(),
            "called enter_serialization_phase but phase is not before 'serialization'"
        );
        self.phase = Phase::Serialization;
    }

    /// Advance to the BeforeTransmit phase.
    #[doc(hidden)]
    pub fn enter_before_transmit_phase(&mut self) {
        debug_assert!(
            self.phase.is_serialization(),
            "called enter_before_transmit_phase but phase is not 'serialization'"
        );
        debug_assert!(
            self.input.is_none(),
            "input must be taken before calling enter_before_transmit_phase"
        );
        debug_assert!(
            self.request.is_some(),
            "request must be set before calling enter_before_transmit_phase"
        );
        self.request_checkpoint = try_clone(self.request());
        self.phase = Phase::BeforeTransmit;
    }

    /// Advance to the Transmit phase.
    #[doc(hidden)]
    pub fn enter_transmit_phase(&mut self) {
        debug_assert!(
            self.phase.is_before_transmit(),
            "called enter_transmit_phase but phase is not before transmit"
        );
        self.phase = Phase::Transmit;
    }

    /// Advance to the BeforeDeserialization phase.
    #[doc(hidden)]
    pub fn enter_before_deserialization_phase(&mut self) {
        debug_assert!(
            self.phase.is_transmit(),
            "called enter_before_deserialization_phase but phase is not 'transmit'"
        );
        debug_assert!(
            self.request.is_none(),
            "request must be taken before entering the 'before deserialization' phase"
        );
        debug_assert!(
            self.response.is_some(),
            "response must be set to before entering the 'before deserialization' phase"
        );
        self.phase = Phase::BeforeDeserialization;
    }

    /// Advance to the Deserialization phase.
    #[doc(hidden)]
    pub fn enter_deserialization_phase(&mut self) {
        debug_assert!(
            self.phase.is_before_deserialization(),
            "called enter_deserialization_phase but phase is not 'before deserialization'"
        );
        self.phase = Phase::Deserialization;
    }

    /// Advance to the AfterDeserialization phase.
    #[doc(hidden)]
    pub fn enter_after_deserialization_phase(&mut self) {
        debug_assert!(
            self.phase.is_deserialization(),
            "called enter_after_deserialization_phase but phase is not 'deserialization'"
        );
        debug_assert!(
            self.output_or_error.is_some(),
            "output must be set to before entering the 'after deserialization' phase"
        );
        self.phase = Phase::AfterDeserialization;
    }

    // Returns false if rewinding isn't possible
    pub fn rewind(&mut self, _cfg: &mut ConfigBag) -> bool {
        // If before transmit was never touched, then we don't need to rewind
        if !self.tainted {
            return true;
        }
        // If request_checkpoint was never set, then this is not a retryable request
        if self.request_checkpoint.is_none() {
            return false;
        }
        // Otherwise, rewind back to the beginning of BeforeTransmit
        // TODO(enableNewSmithyRuntime): Also rewind the ConfigBag
        self.request_checkpoint =
            try_clone(self.request_checkpoint.as_ref().expect("checked above"));
        true
    }
}

impl<I> InterceptorContext<I, TypeErasedBox, TypeErasedError> {
    #[doc(hidden)]
    pub fn fail_with_err(self, err: BoxError) -> SdkError<TypeErasedError, HttpResponse> {
        use Phase::*;
        let Self {
            output_or_error,
            response,
            ..
        } = self;
        match self.phase {
            BeforeSerialization | Serialization => {
                convert_construction_failure(err, output_or_error, response)
            }
            BeforeTransmit | Transmit => convert_dispatch_error(err, output_or_error, response),
            BeforeDeserialization | Deserialization | AfterDeserialization => {
                convert_response_handling_error(err, output_or_error, response)
            }
        }
    }

    #[doc(hidden)]
    pub fn finalize(self) -> Result<TypeErasedBox, SdkError<TypeErasedError, HttpResponse>> {
        match self.output_or_error {
            Some(res) => res
                .map_err(|err| SdkError::service_error(err, self.response.expect("response set"))),
            None => {
                unreachable!("finalize should only be called once output_or_error has been set")
            }
        }
    }
}

fn try_clone(request: &HttpRequest) -> Option<HttpRequest> {
    let cloned_body = request.body().try_clone()?;
    let mut cloned_request = ::http::Request::builder()
        .uri(request.uri().clone())
        .method(request.method());
    *cloned_request
        .headers_mut()
        .expect("builder has not been modified, headers must be valid") = request.headers().clone();
    Some(
        cloned_request
            .body(cloned_body)
            .expect("a clone of a valid request should be a valid request"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_erasure::TypedBox;
    use aws_smithy_http::body::SdkBody;
    use http::header::{AUTHORIZATION, CONTENT_LENGTH};
    use http::{HeaderValue, Uri};

    #[test]
    fn test_success_transitions() {
        let input = TypedBox::new("input".to_string()).erase();
        let output = TypedBox::new("output".to_string()).erase();

        let mut context = InterceptorContext::new(input);
        assert_eq!("input", context.input().downcast_ref::<String>().unwrap());
        context.input_mut();

        context.enter_serialization_phase();
        let _ = context.take_input();
        context.set_request(http::Request::builder().body(SdkBody::empty()).unwrap());

        context.enter_before_transmit_phase();
        context.request();
        context.request_mut();

        context.enter_transmit_phase();
        let _ = context.take_request();
        context.set_response(http::Response::builder().body(SdkBody::empty()).unwrap());

        context.enter_before_deserialization_phase();
        context.response();
        context.response_mut();

        context.enter_deserialization_phase();
        context.response();
        context.response_mut();
        context.set_output_or_error(Ok(output));

        context.enter_after_deserialization_phase();
        context.response();
        context.response_mut();
        let _ = context.output_or_error();
        let _ = context.output_or_error_mut();

        let output = context.output_or_error.unwrap().expect("success");
        assert_eq!("output", output.downcast_ref::<String>().unwrap());
    }

    #[test]
    fn test_rewind_for_retry() {
        use std::fmt;
        #[derive(Debug)]
        struct Error;
        impl fmt::Display for Error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("don't care")
            }
        }
        impl std::error::Error for Error {}

        let mut cfg = ConfigBag::base();
        let input = TypedBox::new("input".to_string()).erase();
        let output = TypedBox::new("output".to_string()).erase();
        let error = TypedBox::new(Error).erase_error();

        let mut context = InterceptorContext::new(input);
        assert_eq!("input", context.input().downcast_ref::<String>().unwrap());

        context.enter_serialization_phase();
        let _ = context.take_input();
        context.set_request(
            http::Request::builder()
                .header("test", "the-original-un-mutated-request")
                .body(SdkBody::empty())
                .unwrap(),
        );
        context.enter_before_transmit_phase();

        // Modify the test header post-checkpoint to simulate modifying the request for signing or a mutating interceptor
        context.request_mut().headers_mut().remove("test");
        context.request_mut().headers_mut().insert(
            "test",
            HeaderValue::from_static("request-modified-after-signing"),
        );

        context.enter_transmit_phase();
        let request = context.take_request();
        assert_eq!(
            "request-modified-after-signing",
            request.headers().get("test").unwrap()
        );
        context.set_response(http::Response::builder().body(SdkBody::empty()).unwrap());

        context.enter_before_deserialization_phase();
        context.enter_deserialization_phase();
        context.set_output_or_error(Err(error));

        assert!(context.rewind(&mut cfg));

        // Now after rewinding, the test header should be its original value
        assert_eq!(
            "the-original-un-mutated-request",
            context.request().headers().get("test").unwrap()
        );

        context.enter_transmit_phase();
        let _ = context.take_request();
        context.set_response(http::Response::builder().body(SdkBody::empty()).unwrap());

        context.enter_before_deserialization_phase();
        context.enter_deserialization_phase();
        context.set_output_or_error(Ok(output));

        context.enter_after_deserialization_phase();

        let output = context.output_or_error.unwrap().expect("success");
        assert_eq!("output", output.downcast_ref::<String>().unwrap());
    }

    #[test]
    fn try_clone_clones_all_data() {
        let request = ::http::Request::builder()
            .uri(Uri::from_static("https://www.amazon.com"))
            .method("POST")
            .header(CONTENT_LENGTH, 456)
            .header(AUTHORIZATION, "Token: hello")
            .body(SdkBody::from("hello world!"))
            .expect("valid request");
        let cloned = try_clone(&request).expect("request is cloneable");

        assert_eq!(&Uri::from_static("https://www.amazon.com"), cloned.uri());
        assert_eq!("POST", cloned.method());
        assert_eq!(2, cloned.headers().len());
        assert_eq!("Token: hello", cloned.headers().get(AUTHORIZATION).unwrap(),);
        assert_eq!("456", cloned.headers().get(CONTENT_LENGTH).unwrap());
        assert_eq!("hello world!".as_bytes(), cloned.body().bytes().unwrap());
    }
}
