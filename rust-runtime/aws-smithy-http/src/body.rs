/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use bytes::Bytes;
use futures::Stream;
use http::{HeaderMap, HeaderValue};
use http_body::{Body, SizeHint};
use pin_project_lite::pin_project;

use std::error::Error as StdError;
use std::fmt::{self, Debug, Formatter};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

pub type Error = Box<dyn StdError + Send + Sync>;

pin_project! {
    /// SdkBody type
    ///
    /// This is the Body used for dispatching all HTTP Requests.
    /// For handling responses, the type of the body will be controlled
    /// by the HTTP stack.
    ///
    /// TODO(naming): Consider renaming to simply `Body`, although I'm concerned about naming headaches
    /// between hyper::Body and our Body
    pub struct SdkBody {
        #[pin]
        inner: Inner,
        // An optional function to recreate the inner body
        //
        // In the event of retry, this function will be called to generate a new body. See
        // [`try_clone()`](SdkBody::try_clone)
        rebuild: Option<Arc<dyn (Fn() -> Inner) + Send + Sync>>,
    }
}

impl Debug for SdkBody {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SdkBody")
            .field("inner", &self.inner)
            .field("retryable", &self.rebuild.is_some())
            .finish()
    }
}

pub type BoxBody = http_body::combinators::BoxBody<Bytes, Error>;

pin_project! {
    #[project = InnerProj]
    enum Inner {
        // An in-memory body
        Once {
            inner: Option<Bytes>
        },
        // A streaming body
        Streaming {
            #[pin]
            inner: hyper::Body
        },
        // Also a streaming body
        Dyn {
            #[pin]
            inner: BoxBody
        },

        /// When a streaming body is transferred out to a stream parser, the body is replaced with
        /// `Taken`. This will return an Error when polled. Attempting to read data out of a `Taken`
        /// Body is a bug.
        Taken,
    }
}

impl Debug for Inner {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self {
            Inner::Once { inner: once } => f.debug_tuple("Once").field(once).finish(),
            Inner::Streaming { inner: streaming } => {
                f.debug_tuple("Streaming").field(streaming).finish()
            }
            Inner::Taken => f.debug_tuple("Taken").finish(),
            Inner::Dyn { .. } => write!(f, "BoxBody"),
        }
    }
}

impl SdkBody {
    /// Construct an SdkBody from a Boxed implementation of http::Body
    pub fn from_dyn(body: BoxBody) -> Self {
        Self {
            inner: Inner::Dyn { inner: body },
            rebuild: None,
        }
    }

    /// Construct an explicitly retryable SDK body
    ///
    /// _Note: This is probably not what you want_
    ///
    /// All bodies constructed from in-memory data (`String`, `Vec<u8>`, `Bytes`, etc.) will be
    /// retryable out of the box. If you want to read data from a file, you should use
    /// [`ByteStream::from_path`](crate::byte_stream::ByteStream::from_path). This function
    /// is only necessary when you need to enable retries for your own streaming container.
    pub fn retryable(f: impl Fn() -> SdkBody + Send + Sync + 'static) -> Self {
        let initial = f();
        SdkBody {
            inner: initial.inner,
            rebuild: Some(Arc::new(move || f().inner)),
        }
    }

    pub fn taken() -> Self {
        Self {
            inner: Inner::Taken,
            rebuild: None,
        }
    }

    pub fn empty() -> Self {
        Self {
            inner: Inner::Once { inner: None },
            rebuild: Some(Arc::new(|| Inner::Once { inner: None })),
        }
    }

    fn poll_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, Error>>> {
        let this = self.project();
        let polling_result = match this.inner.project() {
            InnerProj::Once { ref mut inner } => {
                let data = inner.take();
                match data {
                    Some(bytes) if bytes.is_empty() => Poll::Ready(None),
                    Some(bytes) => Poll::Ready(Some(Ok(bytes))),
                    None => Poll::Ready(None),
                }
            }
            InnerProj::Streaming { inner: body } => body.poll_data(cx).map_err(|e| e.into()),
            InnerProj::Dyn { inner: box_body } => box_body.poll_data(cx),
            InnerProj::Taken => {
                Poll::Ready(Some(Err("A `Taken` body should never be polled".into())))
            }
        };

        polling_result
    }

    /// If possible, return a reference to this body as `&[u8]`
    ///
    /// If this SdkBody is NOT streaming, this will return the byte slab
    /// If this SdkBody is streaming, this will return `None`
    pub fn bytes(&self) -> Option<&[u8]> {
        match &self.inner {
            Inner::Once { inner: Some(b) } => Some(b),
            Inner::Once { inner: None } => Some(&[]),
            _ => None,
        }
    }

    pub fn try_clone(&self) -> Option<Self> {
        self.rebuild.as_ref().map(|rebuild| {
            let next = rebuild();
            Self {
                inner: next,
                rebuild: self.rebuild.clone(),
            }
        })
    }

    pub fn content_length(&self) -> Option<u64> {
        http_body::Body::size_hint(self).exact()
    }

    pub fn map(self, f: impl Fn(SdkBody) -> SdkBody + Sync + Send + 'static) -> SdkBody {
        if self.rebuild.is_some() {
            SdkBody::retryable(move || f(self.try_clone().unwrap()))
        } else {
            f(self)
        }
    }
}

impl From<&str> for SdkBody {
    fn from(s: &str) -> Self {
        Self::from(s.as_bytes())
    }
}

impl From<Bytes> for SdkBody {
    fn from(bytes: Bytes) -> Self {
        SdkBody {
            inner: Inner::Once {
                inner: Some(bytes.clone()),
            },
            rebuild: Some(Arc::new(move || Inner::Once {
                inner: Some(bytes.clone()),
            })),
        }
    }
}

impl From<hyper::Body> for SdkBody {
    fn from(body: hyper::Body) -> Self {
        SdkBody {
            inner: Inner::Streaming { inner: body },
            rebuild: None,
        }
    }
}

impl From<Vec<u8>> for SdkBody {
    fn from(data: Vec<u8>) -> Self {
        Self::from(Bytes::from(data))
    }
}

impl From<String> for SdkBody {
    fn from(s: String) -> Self {
        Self::from(s.into_bytes())
    }
}

impl From<&[u8]> for SdkBody {
    fn from(data: &[u8]) -> Self {
        Self::from(Bytes::copy_from_slice(data))
    }
}

impl http_body::Body for SdkBody {
    type Data = Bytes;
    type Error = Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.poll_inner(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        // `SdkBody`s have no trailers. If returning trailers is necessary,
        // use `SdkBody::map` to wrap this body with another body that does return trailers.
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        match &self.inner {
            Inner::Once { inner: None } => true,
            Inner::Once { inner: Some(bytes) } => bytes.is_empty(),
            Inner::Streaming { inner } => inner.is_end_stream(),
            Inner::Dyn { inner } => inner.is_end_stream(),
            Inner::Taken => true,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.inner {
            Inner::Once { inner: None } => SizeHint::with_exact(0),
            Inner::Once { inner: Some(bytes) } => SizeHint::with_exact(bytes.len() as u64),
            Inner::Streaming { inner } => http_body::Body::size_hint(inner),
            Inner::Dyn { inner } => http_body::Body::size_hint(inner),
            Inner::Taken => SizeHint::new(),
        }
    }
}

const SIZE_HINT_32_BIT_PANIC_MESSAGE: &str = r#"
You're running a 32-bit system and this stream's length is too large to be represented with a usize.
Please limit stream length to less than 4.294Gb or run this program on a 64-bit computer architecture.
"#;

impl Stream for SdkBody {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // By converting to IO errors here, we get access to a bunch of `futures` extension traits.
        // We get access to extension traits from `tokio` too!
        //
        // Convert an `SdkBody` into a `tokio::io::AsyncRead` like so:
        // let body = tokio_util::io::StreamReader::new(SdkBody::from(input));
        //
        // Convert an `SdkBody` into a `futures::io::AsyncRead` like so:
        // let body = SdkBody::from(input).into_async_read();
        self.poll_data(cx).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size_hint = http_body::Body::size_hint(self);
        match (
            size_hint.lower().try_into(),
            size_hint.upper().map(TryInto::try_into).transpose(),
        ) {
            (Ok(lower), Ok(upper)) => (lower, upper),
            (Err(_), _) | (_, Err(_)) => {
                panic!("{}", SIZE_HINT_32_BIT_PANIC_MESSAGE)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::body::{BoxBody, SdkBody};
    use http_body::Body;
    use std::pin::Pin;

    #[test]
    fn valid_size_hint() {
        assert_eq!(SdkBody::from("hello").size_hint().exact(), Some(5));
        assert_eq!(SdkBody::from("").size_hint().exact(), Some(0));
    }

    #[test]
    fn valid_eos() {
        assert_eq!(SdkBody::from("hello").is_end_stream(), false);
        assert_eq!(SdkBody::from("").is_end_stream(), true);
    }

    #[tokio::test]
    async fn http_body_consumes_data() {
        let mut body = SdkBody::from("hello!");
        let mut body = Pin::new(&mut body);
        let data = body.data().await;
        assert!(data.is_some());
        let data = body.data().await;
        assert!(data.is_none());
    }

    #[tokio::test]
    async fn empty_body_returns_none() {
        // Its important to avoid sending empty chunks of data to avoid H2 data frame problems
        let mut body = SdkBody::from("");
        let mut body = Pin::new(&mut body);
        let data = body.data().await;
        assert!(data.is_none());
    }

    #[test]
    fn sdkbody_debug_once() {
        let body = SdkBody::from("123");
        // actually don't really care what the debug impl is, just that it doesn't crash
        let _ = format!("{:?}", body);
    }

    #[test]
    fn sdkbody_debug_dyn() {
        let hyper_body = hyper::Body::channel().1;
        let body = SdkBody::from_dyn(BoxBody::new(hyper_body.map_err(|e| e.into())));
        // actually don't really care what the debug impl is, just that it doesn't crash
        let _ = format!("{:?}", body);
    }

    #[test]
    fn sdkbody_debug_hyper() {
        let hyper_body = hyper::Body::channel().1;
        let body = SdkBody::from(hyper_body);
        // actually don't really care what the debug impl is, just that it doesn't crash
        let _ = format!("{:?}", body);
    }

    #[test]
    fn sdk_body_is_send() {
        fn is_send<T: Send>() {}
        is_send::<SdkBody>()
    }


    #[cfg(feature = "rt-tokio")]
    #[tokio::test]
    async fn sdk_body_can_be_tokio_async_read() {
        use tokio::io::AsyncBufReadExt;

        let body = SdkBody::from("data 1\ndata 2\ndata 3\n");
        let body = tokio_util::io::StreamReader::new(body);
        let async_buf_read = tokio::io::BufReader::new(body);

        let mut lines = async_buf_read.lines();

        assert_eq!(lines.next_line().await.unwrap(), Some("data 1".to_owned()));
        assert_eq!(lines.next_line().await.unwrap(), Some("data 2".to_owned()));
        assert_eq!(lines.next_line().await.unwrap(), Some("data 3".to_owned()));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }

    #[cfg(feature = "rt-tokio")]
    #[tokio::test]
    async fn sdk_body_can_be_futures_async_read() {
        use futures::AsyncReadExt;
        use futures::stream::TryStreamExt;

        let input: &[u8] = b"This is a test body.";
        let mut body = SdkBody::from(input).into_async_read();
        let mut output = [0u8; 20];
        let _ = body.read(&mut output).await.unwrap();

        assert_eq!(&output, input);
    }
}
