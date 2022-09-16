mod handler;
mod layer;
mod request;
mod response;

use aws_smithy_http_server::protocols::Protocol;
use futures::Future;
use http::{Request, Response};
use pyo3_asyncio::TaskLocals;

pub use self::handler::{PyMiddlewareHandler, PyMiddlewareHandlers};
pub use self::layer::PyMiddlewareLayer;
pub use self::request::{PyRequest, PyHttpVersion};
pub use self::response::PyResponse;

pub trait PyMiddlewareTrait<B> {
    type RequestBody;
    type ResponseBody;
    type Future: Future<Output = Result<Request<Self::RequestBody>, Response<Self::ResponseBody>>>;

    fn run(&mut self, request: Request<B>, protocol: Protocol, locals: TaskLocals) -> Self::Future;
}
