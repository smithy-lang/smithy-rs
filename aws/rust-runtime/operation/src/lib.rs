use bytes::Bytes;

use auth::{ProvideCredentials, SigningConfig};

pub mod endpoint;
pub mod middleware;
pub mod signing_middleware;
mod extensions;

use endpoint::ProvideEndpoint;
use http::{HeaderMap, HeaderValue, Response};
use std::error::Error;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::sync::{Arc, Mutex};
use crate::extensions::Extensions;

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

    pub extensions: Extensions,

    // These could also be attached in as extensions, but explicit might be better.
    // Having some explicit configurations as explicit fields doesn't preclude storing data as
    // extensions in the future
    // pub signing_config: SigningConfig,
    // pub credentials_provider: Box<dyn ProvideCredentials>,
    // pub endpoint_config: Box<dyn ProvideEndpoint>,
}

impl Request {

    pub fn new(base: http::Request<SdkBody>) -> Self {
        Request {
            base,
            extensions: Extensions::new()
        }
    }

    pub fn signing_config(&self) -> &SigningConfig {
        self.extensions.get().unwrap()
    }

    pub fn credentials_provider(&self) -> &Box<dyn ProvideCredentials> {
        self.extensions.get().unwrap()
    }

    pub fn endpoint_provider(&self) -> &Box<dyn ProvideEndpoint> {
        self.extensions.get().unwrap()
    }

    pub fn request_mut(&mut self) -> &mut http::Request<SdkBody> {
        &mut self.base
    }

    pub fn add_config<T: Send + Sync + 'static>(&mut self, val: T) -> Option<T> {
        self.extensions.insert(val)
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
