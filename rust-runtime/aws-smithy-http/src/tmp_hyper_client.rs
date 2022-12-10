/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use http::{Request, Response};
use hyper::body::Incoming;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Debug)]
pub struct Client<C, B> {
    _connector: PhantomData<C>,
    _body: PhantomData<B>,
}

impl<C, B> Clone for Client<C, B> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<C, B> Client<C, B> {
    pub fn new() -> Self {
        Client {
            _connector: PhantomData::default(),
            _body: PhantomData::default(),
        }
    }
}

impl<C, B> hyper::service::Service<Request<B>> for Client<C, B> {
    type Response = Response<Incoming>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, req: Request<B>) -> Self::Future {
        todo!()
    }
}

impl<C, B> tower::Service<http::Request<B>> for Client<C, B> {
    type Response = Response<Incoming>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        todo!()
    }
}

#[derive(Debug, Default)]
pub struct Builder {}

impl Builder {
    pub fn build<C, B>(self, _connector: C) -> Client<C, B> {
        Client::new()
    }
}
