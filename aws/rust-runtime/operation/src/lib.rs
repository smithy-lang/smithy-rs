use bytes::Bytes;

use auth::{ProvideCredentials, SigningConfig};

pub mod endpoint;
pub mod middleware;
pub mod signing_middleware;

use endpoint::ProvideEndpoint;
use http::{HeaderMap, HeaderValue};
use std::error::Error;
use std::pin::Pin;
use std::task::{Context, Poll};

type BodyError = Box<dyn Error + Send + Sync>;

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
pub struct Operation<H> {
    pub request: Request,
    pub response_handler: Box<H>,
}

pub struct Request {
    pub base: http::Request<SdkBody>,

    // These could also be attached in as extensions, but explicit might be better.
    // Having some explicit configurations as explicit fields doesn't preclude storing data as
    // extensions in the future
    pub signing_config: SigningConfig,
    pub credentials_provider: Box<dyn ProvideCredentials>,
    pub endpoint_config: Box<dyn ProvideEndpoint>,
}

pub trait ParseHttpResponse {
    type O;
    /// Parse an HTTP request without reading the body. If the body must be provided to proceed,
    /// return `None`
    ///
    /// This exists to serve APIs like S3::GetObject where the body is passed directly into the
    /// response and consumed by the client. However, even in the case of S3::GetObject, errors
    /// require reading the entire body.
    fn parse_unloaded<B: http_body::Body>(
        &self,
        response: &mut http::Response<B>,
    ) -> Option<Self::O>;
    fn parse_loaded(&self, response: &http::Response<Bytes>) -> Self::O;
}
