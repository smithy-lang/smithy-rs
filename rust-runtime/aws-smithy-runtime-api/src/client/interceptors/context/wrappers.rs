/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::InterceptorContext;
use crate::client::interceptors::context::{Request, Response};

pub struct BeforeSerializationInterceptorContextRef<'a, I, O, E> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a InterceptorContext<I, O, E>>
    for BeforeSerializationInterceptorContextRef<'a, I, O, E>
{
    fn from(inner: &'a InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> BeforeSerializationInterceptorContextRef<'a, I, O, E> {
    pub fn input(&self) -> &I {
        self.inner.input()
    }
}

pub struct BeforeSerializationInterceptorContextMut<'a, I, O, E> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a mut InterceptorContext<I, O, E>>
    for BeforeSerializationInterceptorContextMut<'a, I, O, E>
{
    fn from(inner: &'a mut InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> BeforeSerializationInterceptorContextMut<'a, I, O, E> {
    pub fn input_mut(&mut self) -> &mut I {
        self.inner.input_mut()
    }
}

pub struct BeforeTransmitInterceptorContextRef<'a, I, O, E> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a InterceptorContext<I, O, E>>
    for BeforeTransmitInterceptorContextRef<'a, I, O, E>
{
    fn from(inner: &'a InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> BeforeTransmitInterceptorContextRef<'a, I, O, E> {
    pub fn input(&self) -> &I {
        self.inner.input()
    }

    /// Retrieve the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        self.inner.request()
    }
}

pub struct BeforeTransmitInterceptorContextMut<'a, I, O, E> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a mut InterceptorContext<I, O, E>>
    for BeforeTransmitInterceptorContextMut<'a, I, O, E>
{
    fn from(inner: &'a mut InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> BeforeTransmitInterceptorContextMut<'a, I, O, E> {
    pub fn input_mut(&mut self) -> &mut I {
        self.inner.input_mut()
    }
    /// Retrieve the transmittable request for the operation being invoked.
    pub fn request_mut(&mut self) -> &mut Request {
        self.inner.request_mut()
    }
}

pub struct BeforeDeserializationInterceptorContextRef<'a, I, O, E> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a InterceptorContext<I, O, E>>
    for BeforeDeserializationInterceptorContextRef<'a, I, O, E>
{
    fn from(inner: &'a InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> BeforeDeserializationInterceptorContextRef<'a, I, O, E> {
    pub fn input(&self) -> &I {
        self.inner.input()
    }

    /// Retrieve the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        self.inner.request()
    }

    /// Returns the response.
    pub fn response(&self) -> &Response {
        self.inner.response()
    }
}

pub struct BeforeDeserializationInterceptorContextMut<'a, I, O, E> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a mut InterceptorContext<I, O, E>>
    for BeforeDeserializationInterceptorContextMut<'a, I, O, E>
{
    fn from(inner: &'a mut InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> BeforeDeserializationInterceptorContextMut<'a, I, O, E> {
    pub fn input_mut(&mut self) -> &mut I {
        self.inner.input_mut()
    }
    /// Retrieve the transmittable request for the operation being invoked.
    pub fn request_mut(&mut self) -> &mut Request {
        self.inner.request_mut()
    }

    /// Returns a mutable reference to the response.
    pub fn response_mut(&mut self) -> &mut Response {
        self.inner.response_mut()
    }
}

pub struct AfterDeserializationInterceptorContextRef<'a, I, O, E> {
    inner: &'a InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a InterceptorContext<I, O, E>>
    for AfterDeserializationInterceptorContextRef<'a, I, O, E>
{
    fn from(inner: &'a InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> AfterDeserializationInterceptorContextRef<'a, I, O, E> {
    pub fn input(&self) -> &I {
        self.inner.input()
    }

    /// Retrieve the transmittable request for the operation being invoked.
    pub fn request(&self) -> &Request {
        self.inner.request()
    }

    /// Returns the response.
    pub fn response(&self) -> &Response {
        self.inner.response()
    }

    /// Returns the deserialized output or error.
    pub fn output_or_error(&self) -> Result<&O, &E> {
        self.inner.output_or_error()
    }
}

pub struct AfterDeserializationInterceptorContextMut<'a, I, O, E> {
    inner: &'a mut InterceptorContext<I, O, E>,
}

impl<'a, I, O, E> From<&'a mut InterceptorContext<I, O, E>>
    for AfterDeserializationInterceptorContextMut<'a, I, O, E>
{
    fn from(inner: &'a mut InterceptorContext<I, O, E>) -> Self {
        Self { inner }
    }
}

impl<'a, I, O, E> AfterDeserializationInterceptorContextMut<'a, I, O, E> {
    pub fn input_mut(&mut self) -> &mut I {
        self.inner.input_mut()
    }
    /// Retrieve the transmittable request for the operation being invoked.
    pub fn request_mut(&mut self) -> &mut Request {
        self.inner.request_mut()
    }

    /// Returns a mutable reference to the response.
    pub fn response_mut(&mut self) -> &mut Response {
        self.inner.response_mut()
    }
    /// Returns the mutable reference to the deserialized output or error.
    pub fn output_or_error_mut(&mut self) -> &mut Result<O, E> {
        self.inner.output_or_error_mut()
    }
}
