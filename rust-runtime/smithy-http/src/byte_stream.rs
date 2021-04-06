use crate::body;
use crate::body::SdkBody;
use bytes::Buf;
use bytes::{Bytes, BytesMut};
use futures_core::Stream;
use http_body::Body;
use pin_project::pin_project;
use std::fmt::{Debug, Formatter};
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project]
pub struct ByteStream(#[pin] Inner<SdkBody>);

impl Debug for ByteStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ByteStream[{:?}]", self.size_hint())
    }
}

impl ByteStream {
    pub fn new(body: SdkBody) -> Self {
        Self(Inner::new(body))
    }

    pub async fn data(self) -> Result<Bytes, body::Error> {
        self.0.data().await
    }
}

impl futures_core::stream::Stream for ByteStream {
    type Item = Result<Bytes, body::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().0.poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Inner<B> {
    body: B,
}

impl<B> Inner<B> {
    pub fn new(body: B) -> Self {
        Self { body }
    }
    pub async fn data(self) -> Result<Bytes, B::Error>
    where
        B: http_body::Body,
    {
        let sz_hint = http_body::Body::size_hint(&self.body);
        let mut output =
            BytesMut::with_capacity(sz_hint.upper().unwrap_or_else(|| sz_hint.lower()) as usize);
        let body = self.body;
        crate::pin_mut!(body);
        while let Some(buf) = body.data().await {
            let mut buf = buf?;
            while buf.has_remaining() {
                output.extend_from_slice(buf.chunk());
                buf.advance(buf.chunk().len())
            }
        }
        Ok(output.freeze())
    }
}

impl<B> futures_core::stream::Stream for Inner<B>
where
    B: http_body::Body + Unpin,
{
    type Item = Result<Bytes, B::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let body = &mut self.body;
        crate::pin_mut!(body);
        match body.poll_data(cx) {
            Poll::Ready(Some(Ok(mut data))) => {
                let len = data.chunk().len();
                let bytes = data.copy_to_bytes(len);
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size_hint = http_body::Body::size_hint(&self.body);
        (
            size_hint.lower() as usize,
            size_hint.upper().map(|u| u as usize),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::byte_stream::Inner;
    use bytes::buf::Chain;
    use bytes::{Buf, Bytes};
    use hyper::http::HeaderValue;
    use hyper::HeaderMap;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[tokio::test]
    async fn read_from_string_body() {
        let body = hyper::Body::from("a simple body");
        assert_eq!(
            Inner::new(body).data().await.expect("no errors"),
            Bytes::from("a simple body")
        );
    }

    #[tokio::test]
    async fn read_from_channel_body() {
        let (mut sender, body) = hyper::Body::channel();
        let byte_stream = Inner::new(body);
        tokio::spawn(async move {
            sender.send_data(Bytes::from("data 1")).await.unwrap();
            sender.send_data(Bytes::from("data 2")).await.unwrap();
            sender.send_data(Bytes::from("data 3")).await.unwrap();
        });
        assert_eq!(
            byte_stream.data().await.expect("no errors"),
            Bytes::from("data 1data 2data 3")
        );
    }

    #[tokio::test]
    async fn read_from_chain_body() {
        // This test exists to validate that we are reading to the end of the individual `Buf`s
        // that come out of `http_body::Body`. An initial implementation did not.
        type NestedChain = Chain<Bytes, Chain<bytes::Bytes, bytes::Bytes>>;
        struct ChainBody(Vec<NestedChain>);
        impl ChainBody {
            fn poll_inner(&mut self) -> Poll<Option<Result<NestedChain, ()>>> {
                Poll::Ready(self.0.pop().map(|b| Ok(b)))
            }
        }
        impl http_body::Body for ChainBody {
            type Data = NestedChain;
            type Error = ();

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
                unimplemented!()
            }
        }

        let chain1 = Bytes::from("a").chain(Bytes::from("b").chain(Bytes::from("c")));
        let chain2 = Bytes::from("c").chain(Bytes::from("d").chain(Bytes::from("e")));

        let body = ChainBody(vec![chain2, chain1]);
        assert_eq!(
            Inner::new(body).data().await.expect("no errors"),
            Bytes::from("abccde")
        );
    }
}
