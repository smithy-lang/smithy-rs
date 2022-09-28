/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use tower::Layer;

use super::{InstrumentOperation, MakeIdentity};

/// A [`Layer`] used to apply [`InstrumentOperation`].
#[derive(Debug)]
pub struct InstrumentLayer<RequestMakeFmt = MakeIdentity, ResponseMakeFmt = MakeIdentity> {
    operation_name: &'static str,
    make_request: RequestMakeFmt,
    make_response: ResponseMakeFmt,
}

impl InstrumentLayer {
    /// Constructs a new [`InstrumentLayer`] with no data redacted.
    pub fn new(operation_name: &'static str) -> Self {
        Self {
            operation_name,
            make_request: MakeIdentity,
            make_response: MakeIdentity,
        }
    }
}

impl<RequestMakeFmt, ResponseMakeFmt> InstrumentLayer<RequestMakeFmt, ResponseMakeFmt> {
    /// Configures the request format.
    ///
    /// The argument is typically [`RequestFmt`](super::sensitivity::RequestFmt).
    pub fn request_fmt<R>(self, make_request: R) -> InstrumentLayer<R, ResponseMakeFmt> {
        InstrumentLayer {
            operation_name: self.operation_name,
            make_request,
            make_response: self.make_response,
        }
    }

    /// Configures the response format.
    ///
    /// The argument is typically [`ResponseFmt`](super::sensitivity::ResponseFmt).
    pub fn response_fmt<R>(self, make_response: R) -> InstrumentLayer<RequestMakeFmt, R> {
        InstrumentLayer {
            operation_name: self.operation_name,
            make_request: self.make_request,
            make_response,
        }
    }
}

impl<S, RequestMakeFmt, ResponseMakeFmt> Layer<S> for InstrumentLayer<RequestMakeFmt, ResponseMakeFmt>
where
    RequestMakeFmt: Clone,
    ResponseMakeFmt: Clone,
{
    type Service = InstrumentOperation<S, RequestMakeFmt, ResponseMakeFmt>;

    fn layer(&self, service: S) -> Self::Service {
        InstrumentOperation::new(service, self.operation_name)
            .request_fmt(self.make_request.clone())
            .response_fmt(self.make_response.clone())
    }
}
