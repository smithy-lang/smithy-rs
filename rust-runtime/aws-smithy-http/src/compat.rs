/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use core::task::ready;
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

pin_project! {
    /// http-0.4.x to http-1.x body conversion util. This is meant for internal use only and may be
    /// removed in the future.
    pub struct Http04toHttp1x<B> {
        #[pin]
        inner: B,
        state: State,
    }
}

enum State {
    PollData,
    PollTrailers,
    Done,
}

impl<B> Http04toHttp1x<B> {
    /// Create a new shim to convert http-0.4.x body to an http-1.x body.
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            state: State::PollData,
        }
    }
}

impl<B> http_body_1x::Body for Http04toHttp1x<B>
where
    B: http_body_04x::Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        loop {
            match this.state {
                State::PollData => match ready!(this.inner.as_mut().poll_data(cx)) {
                    Some(data) => return Poll::Ready(Some(data.map(http_body_1x::Frame::data))),
                    None => *this.state = State::PollTrailers,
                },
                State::PollTrailers => {
                    let result = match ready!(this.inner.as_mut().poll_trailers(cx)) {
                        Ok(trailers) => Poll::Ready(
                            trailers
                                .map(convert_headers_0x_1x)
                                .map(http_body_1x::Frame::<Self::Data>::trailers)
                                .map(Result::Ok),
                        ),
                        Err(err) => Poll::Ready(Some(Err(err))),
                    };

                    *this.state = State::Done;
                    return result;
                }
                State::Done => return Poll::Ready(None),
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body_1x::SizeHint {
        let mut size_hint = http_body_1x::SizeHint::new();
        let inner_hint = self.inner.size_hint();
        if let Some(exact) = inner_hint.exact() {
            size_hint.set_exact(exact)
        } else {
            size_hint.set_lower(inner_hint.lower());
            if let Some(upper) = inner_hint.upper() {
                size_hint.set_upper(upper);
            }
        }
        size_hint
    }
}

pub fn convert_headers_0x_1x(input: http_02x::HeaderMap) -> http_1x::HeaderMap {
    let mut map = http_1x::HeaderMap::with_capacity(input.capacity());
    let mut mem: Option<http_02x::HeaderName> = None;
    for (k, v) in input.into_iter() {
        let name = k.or_else(|| mem.clone()).unwrap();
        map.append(
            http_1x::HeaderName::from_bytes(name.as_str().as_bytes()).expect("already validated"),
            http_1x::HeaderValue::from_bytes(v.as_bytes()).expect("already validated"),
        );
        mem = Some(name);
    }
    map
}

pin_project! {
    /// http-1.x to http-0.4.x body conversion util. This is meant for internal use only and may be
    /// removed in the future.
    pub struct Http1toHttp04<B> {
        #[pin]
        inner: B,
        trailers: Option<http_1x::HeaderMap>,
    }
}

impl<B> Http1toHttp04<B> {
    /// Create a new shim to convert http-1.x body to an http-0.4.x body.
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            trailers: None,
        }
    }
}

impl<B> http_body_04x::Body for Http1toHttp04<B>
where
    B: http_body_1x::Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        loop {
            let this = self.as_mut().project();
            match ready!(this.inner.poll_frame(cx)) {
                Some(Ok(frame)) => {
                    let frame = match frame.into_data() {
                        Ok(data) => return Poll::Ready(Some(Ok(data))),
                        Err(frame) => frame,
                    };
                    // when we get a trailers frame, store the trailers for the next poll
                    if let Ok(trailers) = frame.into_trailers() {
                        this.trailers.replace(trailers);
                        return Poll::Ready(None);
                    };
                    // if the frame type was unknown, discard it. the next one might be something
                    // useful
                }
                Some(Err(e)) => return Poll::Ready(Some(Err(e))),
                None => return Poll::Ready(None),
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http_02x::HeaderMap>, Self::Error>> {
        // all of the polling happens in poll_data, once we get to the trailers we've actually
        // already read everything
        let this = self.project();
        match this.trailers.take() {
            Some(headers) => Poll::Ready(Ok(Some(convert_headers_1x_0x(headers)))),
            None => Poll::Ready(Ok(None)),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body_04x::SizeHint {
        let mut size_hint = http_body_04x::SizeHint::new();
        let inner_hint = self.inner.size_hint();
        if let Some(exact) = inner_hint.exact() {
            size_hint.set_exact(exact);
        } else {
            size_hint.set_lower(inner_hint.lower());
            if let Some(upper) = inner_hint.upper() {
                size_hint.set_upper(upper);
            }
        }
        size_hint
    }
}

pub fn convert_headers_1x_0x(input: http_1x::HeaderMap) -> http_02x::HeaderMap {
    let mut map = http_02x::HeaderMap::with_capacity(input.capacity());
    let mut mem: Option<http_1x::HeaderName> = None;
    for (k, v) in input.into_iter() {
        let name = k.or_else(|| mem.clone()).unwrap();
        map.append(
            http_02x::HeaderName::from_bytes(name.as_str().as_bytes()).expect("already validated"),
            http_02x::HeaderValue::from_bytes(v.as_bytes()).expect("already validated"),
        );
        mem = Some(name);
    }
    map
}
