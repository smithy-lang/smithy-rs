/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use bytes::Bytes;
use http::{HeaderMap, HeaderValue};
use std::error::Error;
use std::pin::Pin;
use std::task::{Context, Poll};

type BodyError = Box<dyn Error + Send + Sync>;

/// SdkBody type
///
/// This is the Body used for dispatching all HTTP Requests.
/// For handling responses, the type of the body will be controlled
/// by the HTTP stack.
///
/// TODO: Consider renaming to simply `Body`, although I'm concerned about naming headaches
/// between hyper::Body and our Body
pub enum SdkBody {
    Once(Option<Bytes>),
    // TODO: tokio::sync::mpsc based streaming body
}

impl SdkBody {
    fn poll_inner(&mut self) -> Poll<Option<Result<Bytes, BodyError>>> {
        match self {
            SdkBody::Once(ref mut opt) => {
                let data = opt.take();
                match data {
                    Some(bytes) => Poll::Ready(Some(Ok(bytes))),
                    None => Poll::Ready(None),
                }
            }
        }
    }
}

impl From<&str> for SdkBody {
    fn from(s: &str) -> Self {
        SdkBody::Once(Some(Bytes::copy_from_slice(s.as_bytes())))
    }
}

impl From<Bytes> for SdkBody {
    fn from(bytes: Bytes) -> Self {
        SdkBody::Once(Some(bytes))
    }
}

impl From<Vec<u8>> for SdkBody {
    fn from(data: Vec<u8>) -> SdkBody {
        Self::from(Bytes::from(data))
    }
}

impl http_body::Body for SdkBody {
    type Data = Bytes;
    type Error = BodyError;

    fn poll_data(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.poll_inner()
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}
