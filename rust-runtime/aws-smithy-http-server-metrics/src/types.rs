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

pub(crate) type DefaultInit<E, S> = fn() -> AppendAndCloseOnDrop<E, S>;
pub(crate) type DefaultRq<E> = fn(&mut Request<ReqBody>, &mut E);
pub(crate) type DefaultRs<E> = fn(&mut Response<ResBody>, &mut E);
