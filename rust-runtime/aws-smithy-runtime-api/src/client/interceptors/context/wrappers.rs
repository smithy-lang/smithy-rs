/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{Error, Input, InterceptorContext, Output};
use crate::client::interceptors::context::{Request, Response};
use crate::client::orchestrator::OrchestratorError;
use std::fmt::Debug;

macro_rules! impl_from_interceptor_context {
    (ref $wrapper:ident) => {
        impl<'a, I, O, E> From<&'a InterceptorContext<I, O, E>> for $wrapper<'a, I, O, E> {
            fn from(inner: &'a InterceptorContext<I, O, E>) -> Self {
                Self { inner }
            }
        }
    };
    (mut $wrapper:ident) => {
        impl<'a, I, O, E> From<&'a mut InterceptorContext<I, O, E>> for $wrapper<'a, I, O, E> {
            fn from(inner: &'a mut InterceptorContext<I, O, E>) -> Self {
                Self { inner }
            }
        }
    };
}

macro_rules! expect {
    ($self:ident, $what:ident) => {
        $self.inner.$what().expect(concat!(
            "`",
            stringify!($what),
            "` wasn't set in the underlying interceptor context. This is a bug."
        ))
    };
}

//
// BeforeSerializationInterceptorContextRef
//

#[derive(Debug)]
pub struct BeforeSerializationInterceptorContextRef<'a, I = Input, O = Output, E = Error> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(ref BeforeSerializationInterceptorContextRef);

impl<'a, I, O, E> BeforeSerializationInterceptorContextRef<'a, I, O, E> {
    /// Returns a reference to the input.
    pub fn input(&self) -> &I {
        expect!(self, input)
    }
}

//
// BeforeSerializationInterceptorContextMut
//

#[derive(Debug)]
pub struct BeforeSerializationInterceptorContextMut<'a, I = Input, O = Output, E = Error> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(mut BeforeSerializationInterceptorContextMut);

impl<'a, I, O, E> BeforeSerializationInterceptorContextMut<'a, I, O, E> {
    /// Returns a reference to the input.
    pub fn input(&self) -> &I {
        expect!(self, input)
    }

    /// Returns a mutable reference to the input.
    pub fn input_mut(&mut self) -> &mut I {
        expect!(self, input_mut)
    }
}

//
// BeforeSerializationInterceptorContextRef
//

#[derive(Debug)]
pub struct BeforeTransmitInterceptorContextRef<'a, I = Input, O = Output, E = Error> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(ref BeforeTransmitInterceptorContextRef);

impl<'a, I, O, E> BeforeTransmitInterceptorContextRef<'a, I, O, E> {
    /// Returns a reference to the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        expect!(self, request)
    }
}

//
// BeforeSerializationInterceptorContextMut
//

#[derive(Debug)]
pub struct BeforeTransmitInterceptorContextMut<'a, I = Input, O = Output, E = Error> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(mut BeforeTransmitInterceptorContextMut);

impl<'a, I, O, E> BeforeTransmitInterceptorContextMut<'a, I, O, E> {
    /// Returns a reference to the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        expect!(self, request)
    }

    /// Returns a mutable reference to the transmittable request for the operation being invoked.
    pub fn request_mut(&mut self) -> &mut Request {
        expect!(self, request_mut)
    }
}

//
// BeforeDeserializationInterceptorContextRef
//

#[derive(Debug)]
pub struct BeforeDeserializationInterceptorContextRef<'a, I = Input, O = Output, E = Error> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(ref BeforeDeserializationInterceptorContextRef);

impl<'a, I, O, E> BeforeDeserializationInterceptorContextRef<'a, I, O, E> {
    /// Returns a reference to the input.
    pub fn input(&self) -> &I {
        expect!(self, input)
    }

    /// Returns a reference to the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        expect!(self, request)
    }

    /// Returns a reference to the response.
    pub fn response(&self) -> &Response {
        expect!(self, response)
    }
}

//
// BeforeDeserializationInterceptorContextMut
//

pub struct BeforeDeserializationInterceptorContextMut<'a, I = Input, O = Output, E = Error> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(mut BeforeDeserializationInterceptorContextMut);

impl<'a, I, O, E> BeforeDeserializationInterceptorContextMut<'a, I, O, E> {
    /// Returns a reference to the input.
    pub fn input(&self) -> &I {
        expect!(self, input)
    }

    /// Returns a reference to the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        expect!(self, request)
    }

    /// Returns a reference to the response.
    pub fn response(&self) -> &Response {
        expect!(self, response)
    }

    /// Returns a mutable reference to the input.
    pub fn input_mut(&mut self) -> &mut I {
        expect!(self, input_mut)
    }

    /// Returns a mutable reference to the transmittable request for the operation being invoked.
    pub fn request_mut(&mut self) -> &mut Request {
        expect!(self, request_mut)
    }

    /// Returns a mutable reference to the response.
    pub fn response_mut(&mut self) -> &mut Response {
        expect!(self, response_mut)
    }

    #[doc(hidden)]
    /// Downgrade this helper struct, returning the underlying InterceptorContext. There's no good
    /// reason to use this unless you're writing tests or you have to interact with an API that
    /// doesn't support the helper structs.
    pub fn inner_mut(&mut self) -> &'_ mut InterceptorContext<I, O, E> {
        self.inner
    }
}

//
// AfterDeserializationInterceptorContextRef
//

pub struct AfterDeserializationInterceptorContextRef<'a, I = Input, O = Output, E = Error> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(ref AfterDeserializationInterceptorContextRef);

impl<'a, I, O, E> AfterDeserializationInterceptorContextRef<'a, I, O, E> {
    /// Returns a reference to the input.
    pub fn input(&self) -> &I {
        expect!(self, input)
    }

    /// Returns a reference to the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        expect!(self, request)
    }

    /// Returns a reference to the response.
    pub fn response(&self) -> &Response {
        expect!(self, response)
    }

    /// Returns a reference to the deserialized output or error.
    pub fn output_or_error(&self) -> Result<&O, &OrchestratorError<E>> {
        expect!(self, output_or_error)
    }
}

//
// FinalizerInterceptorContextRef
//

pub struct FinalizerInterceptorContextRef<'a, I = Input, O = Output, E = Error> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(ref FinalizerInterceptorContextRef);

impl<'a, I, O, E> FinalizerInterceptorContextRef<'a, I, O, E> {
    /// Returns the operation input.
    pub fn input(&self) -> Option<&I> {
        self.inner.input.as_ref()
    }

    /// Returns the serialized request.
    pub fn request(&self) -> Option<&Request> {
        self.inner.request.as_ref()
    }

    /// Returns the raw response.
    pub fn response(&self) -> Option<&Response> {
        self.inner.response.as_ref()
    }

    /// Returns the deserialized operation output or error.
    pub fn output_or_error(&self) -> Option<Result<&O, &OrchestratorError<E>>> {
        self.inner.output_or_error.as_ref().map(|o| o.as_ref())
    }
}

//
// FinalizerInterceptorContextMut
//

pub struct FinalizerInterceptorContextMut<'a, I = Input, O = Output, E = Error> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl_from_interceptor_context!(mut FinalizerInterceptorContextMut);

impl<'a, I, O, E> FinalizerInterceptorContextMut<'a, I, O, E> {
    /// Returns the operation input.
    pub fn input(&self) -> Option<&I> {
        self.inner.input.as_ref()
    }

    /// Returns the serialized request.
    pub fn request(&self) -> Option<&Request> {
        self.inner.request.as_ref()
    }

    /// Returns the raw response.
    pub fn response(&self) -> Option<&Response> {
        self.inner.response.as_ref()
    }

    /// Returns the deserialized operation output or error.
    pub fn output_or_error(&self) -> Option<Result<&O, &OrchestratorError<E>>> {
        self.inner.output_or_error.as_ref().map(|o| o.as_ref())
    }

    /// Mutably returns the operation input.
    pub fn input_mut(&mut self) -> Option<&mut I> {
        self.inner.input.as_mut()
    }

    /// Mutably returns the serialized request.
    pub fn request_mut(&mut self) -> Option<&mut Request> {
        self.inner.request.as_mut()
    }

    /// Mutably returns the raw response.
    pub fn response_mut(&mut self) -> Option<&mut Response> {
        self.inner.response.as_mut()
    }

    /// Mutably returns the deserialized operation output or error.
    pub fn output_or_error_mut(&mut self) -> Option<&mut Result<O, OrchestratorError<E>>> {
        self.inner.output_or_error.as_mut()
    }
}
