use bytes::Bytes;

pub mod endpoint;
mod extensions;
pub mod middleware;
pub mod signing_middleware;
pub mod retry_policy;

use crate::extensions::Extensions;
use http::{HeaderMap, HeaderValue, Response};
use std::error::Error;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

type BodyError = Box<dyn Error + Send + Sync>;

#[derive(Debug)]
pub enum SdkBody {
    Once(Option<Bytes>),
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

    pub fn try_clone(&self) -> Option<SdkBody> {
        match self {
            SdkBody::Once(bytes) => Some(SdkBody::Once(bytes.clone())),
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

// TODO: consider field privacy, builders, etc.
pub struct Operation<H, R> {
    pub request: Request,
    pub response_handler: H,
    pub retry_policy: R
}

impl<H> Operation<H, ()> {
    pub fn new(request: Request, response_handler: H) -> Self {
        Operation {
            request,
            response_handler,
            retry_policy: ()
        }
    }
}

pub struct Request {
    pub base: http::Request<SdkBody>,
    pub config: Arc<Mutex<Extensions>>,
}

impl Request {
    pub fn new(base: http::Request<SdkBody>) -> Self {
        Request {
            base,
            config: Arc::new(Mutex::new(Extensions::new())),
        }
    }

    pub fn augment<T>(
        self,
        f: impl FnOnce(http::Request<SdkBody>, &Extensions) -> Result<http::Request<SdkBody>, T>,
    ) -> Result<Request, T> {
        let base = {
            let extensions = (&self.config).lock().unwrap();
            f(self.base, &extensions)?
        };
        Ok(Request {
            base,
            config: self.config,
        })
    }

    pub fn try_clone(&self) -> Option<Request> {
        let cloned_body = self.base.body().try_clone()?;
        let mut cloned_request = http::Request::builder()
            .uri(self.base.uri().clone())
            .method(self.base.method());
        for (name, value) in self.base.headers() {
            cloned_request = cloned_request.header(name, value)
        }
        let base = cloned_request
            .body(cloned_body)
            .expect("should be clonable");
        Some(Request {
            base,
            config: self.config.clone(),
        })
    }
}

pub trait ParseHttpResponse<B> {
    type Output;
    /// Parse an HTTP request without reading the body. If the body must be provided to proceed,
    /// return `None`
    ///
    /// This exists to serve APIs like S3::GetObject where the body is passed directly into the
    /// response and consumed by the client. However, even in the case of S3::GetObject, errors
    /// require reading the entire body.
    fn parse_unloaded(&self, response: &mut http::Response<B>) -> Option<Self::Output>;
    fn parse_loaded(&self, response: &http::Response<Bytes>) -> Self::Output;
}

pub trait ParseStrictResponse {
    type Output;
    fn parse(&self, response: &Response<Bytes>) -> Self::Output;
}

impl<B, T> ParseHttpResponse<B> for T
where
    T: ParseStrictResponse,
{
    type Output = T::Output;

    fn parse_unloaded(&self, _response: &mut Response<B>) -> Option<Self::Output> {
        None
    }

    fn parse_loaded(&self, response: &Response<Bytes>) -> Self::Output {
        self.parse(response)
    }
}
