RFC: Implementing `tokio::io::AsyncRead` for `aws_smithy_http::byte_stream::ByteStream`
=================================================

> Status: RFC

Customers have expressed the desire that [`aws_smithy_http::byte_stream::ByteStream`][ByteStream] should implement the [`tokio::io::AsyncRead`][AsyncRead] trait. This would enable them to make use of existing tools for decompressing and buffering data:

```rust
let data: Box<dyn AsyncRead> = Box::new(some_bytestream);
// Read a gzipped text file line by line
for line in BufReader::new(GzipDecoder::new(data))).lines() {
  println!("{line}");
}
```

Users currently have the option of wrapping `ByteStream` in a newtype and implementing the `AsyncRead` trait themselves. This is not a good customer experience for reasons of ergonomics and performance that we'll cover later in this RFC.

## Implementing `AsyncRead` for a newtype

Here's how we could implement a solution with a newtype:

```rust
use super::ByteStream;
use bytes::Bytes;
use pin_project::pin_project;
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project]
pub struct BufferedByteStream {
    #[pin]
    inner: ByteStream,
    #[pin]
    buffer: VecDeque<Bytes>,
}

impl BufferedByteStream {
    pub fn new(bytestream: ByteStream) -> Self {
        Self {
            inner: bytestream,
            buffer: VecDeque::new(),
        }
    }
}

impl From<ByteStream> for BufferedByteStream {
    fn from(bytestream: ByteStream) -> Self {
        Self::new(bytestream)
    }
}

fn fill_buf_and_capture_spillover(
    buf: &mut tokio::io::ReadBuf<'_>,
    spillover_buf: &mut VecDeque<Bytes>,
    data: Option<Bytes>,
) {
    // We won't have new data when recursing
    if let Some(data) = data {
        spillover_buf.push_back(data);
    }
    if let Some(mut next_bytes) = spillover_buf.pop_front() {
        if next_bytes.len() == buf.remaining() {
            // Perfect fit! Put the `next_bytes` into the buf
            buf.put_slice(next_bytes.as_ref())
        } else if next_bytes.len() < buf.remaining() {
            // Else if `next_bytes` is too small we'll recurse
            buf.put_slice(next_bytes.as_ref());
            fill_buf_and_capture_spillover(buf, spillover_buf, None);
        } else {
            // Else, if `next_bytes` is too big to fit in `buf`, split `next_bytes` into two pieces. The
            // first piece will be exactly big enough to fit in `buf`. We'll put the leftover piece back
            // in the `spillover_buf`
            let front_bytes = next_bytes.split_to(buf.remaining());
            assert_eq!(front_bytes.len(), buf.remaining());

            buf.put_slice(front_bytes.as_ref());

            // Put the leftover bytes back in the queue
            spillover_buf.push_front(next_bytes);
        }
    }
}

impl tokio::io::AsyncRead for BufferedByteStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        use futures_core::Stream;

        let mut this = self.project();
        match this.inner.poll_next(cx) {
            Poll::Ready(Some(Ok(data))) => {
                fill_buf_and_capture_spillover(buf, &mut this.buffer, Some(data));

                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                // Check if any data remains in the buffer and read it if necessary
                // If data is still remaining in the buffer,
                if !this.buffer.is_empty() {
                    fill_buf_and_capture_spillover(buf, &mut this.buffer, None);
                }

                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Err(tokio::io::Error::new(tokio::io::ErrorKind::Other, e)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod test {
    use super::{BufferedByteStream, ByteStream};
    use bytes::BytesMut;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_buffered_byte_stream_doesnt_drop_bytes_with_exact_size_buffer() {
        let input = "This is a test";
        let bs = ByteStream::from_static(input.as_bytes());
        let mut bbs = BufferedByteStream::from(bs);
        let mut buffer = BytesMut::with_capacity(14);
        assert!(buffer.is_empty());
        let bytes_read = bbs.read_buf(&mut buffer).await.unwrap();
        assert_eq!(14, bytes_read);
        let output = std::str::from_utf8(&buffer).unwrap();
        assert_eq!(input, output)
    }

    pub fn bytes_remaining_in_buffer(buffered_byte_stream: &BufferedByteStream) -> usize {
        buffered_byte_stream
            .buffer
            .iter()
            .fold(0, |acc, next| acc + next.len())
    }

    #[tokio::test]
    async fn test_buffered_byte_stream_doesnt_drop_bytes_with_smaller_buffer() {
        let input = "This is a test";
        let bs = ByteStream::from_static(input.as_bytes());
        let mut bbs = BufferedByteStream::from(bs);
        let mut buffer = BytesMut::with_capacity(5);

        // Nothing in internal buffer b/c we haven't read anything yet
        assert_eq!(0, bytes_remaining_in_buffer(&bbs));

        let bytes_read = bbs.read_buf(&mut buffer).await.unwrap();
        // buffer is 5 bytes big so there should be 9 bytes left in the buffer
        assert_eq!(9, bytes_remaining_in_buffer(&bbs));
        assert_eq!(5, bytes_read);
        let output = std::str::from_utf8(&buffer).unwrap();
        assert_eq!("This ", output);
        buffer.clear();

        let bytes_read = bbs.read_buf(&mut buffer).await.unwrap();
        assert_eq!(4, bytes_remaining_in_buffer(&bbs));
        assert_eq!(5, bytes_read);
        let output = std::str::from_utf8(&buffer).unwrap();
        assert_eq!("is a ", output);
        buffer.clear();

        let bytes_read = bbs.read_buf(&mut buffer).await.unwrap();
        assert_eq!(0, bytes_remaining_in_buffer(&bbs));
        assert_eq!(4, bytes_read);
        let output = std::str::from_utf8(&buffer).unwrap();
        assert_eq!("test", output);
        buffer.clear();
    }

    #[tokio::test]
    async fn test_buffered_byte_stream_works_with_very_large_buffer() {
        let input = "This is a test";
        let bs = ByteStream::from_static(input.as_bytes());
        let mut bbs = BufferedByteStream::from(bs);
        let mut buffer = BytesMut::with_capacity(9000);
        assert!(buffer.is_empty());
        let bytes_read = bbs.read_buf(&mut buffer).await.unwrap();
        assert_eq!(14, bytes_read);
        let output = std::str::from_utf8(&buffer).unwrap();
        assert_eq!(input, output)
    }
}
```

## Is the newtype solution enough?

The newtype approach doesn't allow for much optimisation. `ByteStream`s might be constructed from `tokio::fs::File`s which already implement `AsyncRead`. In that case, it would be much better to delegate directly to the inner `File`.

The above solution doesn't allow for backpressure and the buffer size isn't configurable but these features could probably be added.

[ByteStream]: https://docs.rs/aws-smithy-http/latest/aws_smithy_http/byte_stream/struct.ByteStream.html
[AsyncRead]: https://docs.rs/tokio/latest/tokio/io/trait.AsyncRead.html
