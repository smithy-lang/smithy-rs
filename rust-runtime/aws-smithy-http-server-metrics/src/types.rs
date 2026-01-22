/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::error::Error;
use http::Request;
use http::Response;
use http_body::combinators::UnsyncBoxBody;
use hyper::body::Bytes;
use metrique::AppendAndCloseOnDrop;

pub type ReqBody = hyper::body::Body;
pub type ResBody = UnsyncBoxBody<Bytes, Error>;

pub(crate) type DefaultInit<E, S> = fn() -> AppendAndCloseOnDrop<E, S>;
pub(crate) type DefaultRq<E> = fn(&mut Request<ReqBody>, &mut E);
pub(crate) type DefaultRs<E> = fn(&mut Response<ResBody>, &mut E);
