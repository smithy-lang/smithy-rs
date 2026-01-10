#![allow(missing_docs)]

use aws_smithy_http_server::error::Error;
use http_body::combinators::UnsyncBoxBody;
use hyper::body::Bytes;

pub mod default;
pub mod layer;
pub mod plugin;
pub mod service;

type ReqBody = hyper::body::Body;
type ResBody = UnsyncBoxBody<Bytes, Error>;
