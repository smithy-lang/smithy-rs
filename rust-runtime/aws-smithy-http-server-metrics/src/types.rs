/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::body::BoxBody;
use http::Request;
use http::Response;
use hyper::body::Incoming;
use metrique::AppendAndCloseOnDrop;

/// Re-exported type for [`hyper::body::Incoming`]
pub type ReqBody = Incoming;

/// Re-exported type for [`aws_smithy_http_server::body::BoxBody`]
pub type ResBody = BoxBody;

pub(crate) type DefaultInit<Entry, Sink> =
    fn(&mut Request<ReqBody>) -> AppendAndCloseOnDrop<Entry, Sink>;
pub(crate) type DefaultRs<Entry> = fn(&mut Response<ResBody>, &mut Entry);
