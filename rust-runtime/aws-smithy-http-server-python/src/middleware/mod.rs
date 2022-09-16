mod handler;
mod layer;
mod request;
mod response;

use aws_smithy_http_server::body::{Body, BoxBody};
use aws_smithy_http_server::protocols::Protocol;
use futures::Future;
use futures::future::BoxFuture;
use http::{Request, Response};
use pyo3_asyncio::TaskLocals;

pub use self::handler::{PyMiddlewareHandler, PyMiddlewares};
pub use self::layer::PyMiddlewareLayer;
pub use self::request::{PyRequest, PyHttpVersion};
pub use self::response::PyResponse;

pub type PyFuture = BoxFuture<'static, Result<Request<Body>, Response<BoxBody>>>;
