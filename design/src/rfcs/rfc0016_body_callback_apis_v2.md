RFC: Replacing Callback APIs for `ByteStream` and `SdkBody` with a body-wrapping API
=================================================

> Status: RFC

Adding a body-wrapping API to `ByteStream` and `SdkBody` will enable developers using the SDK to implement things like checksum validations and 'read progress' callbacks. This RFC supersedes [RFC0013] and will partially replace the changes made for that RFC.

## The Implementation

*Note that comments starting with '//' are not necessarily going to be included in the actual implementation and are intended as clarifying comments for the purposes of this RFC.*

The changes we need to make to `ByteStream`:

*(The current version of `ByteStream` and `Inner` can be seen [here][ByteStream impls].)*

```rust
// in `aws_smithy_http::byte_stream`...

// We add a new method to `ByteStream` for wrapping the body
impl ByteStream {
    // ...other impls omitted

    pub fn wrap_body(self, wrapper_fn: impl FnOnce(SdkBody) -> SdkBody) -> Self {
        let inner_body = self.into_inner();
        let new_inner = wrapper_fn(inner_body);

        ByteStream::new(new_inner)
    }
}
```

Changes made to `byte_stream::Inner` and `SdkBody` as part of [RFC0013] will be reverted. They're no longer necessary with this new approach.

*(The current version of `SdkBody` can be seen [here][SdkBody impls].)*

## Implementing Checksums

Checksum callbacks were introduced after the acceptance of [RFC0013] and will be modified only slightly. A new `ChecksumBody` will be implemented that can wrap an `SdkBody`, providing checksum trailers and size hints that take those trailers into account.

```rust
// In aws-smithy-checksums/src/lib.rs
use aws_smithy_types::base64;

use http::header::{HeaderMap, HeaderName, HeaderValue};
use http_body::SizeHint;
use sha1::Digest;
use std::io::Write;

pub mod body;

pub const CRC_32_NAME: &str = "crc32";
pub const CRC_32_C_NAME: &str = "crc32c";
pub const SHA_1_NAME: &str = "sha1";
pub const SHA_256_NAME: &str = "sha256";

const CRC_32_HEADER_NAME: &str = "x-amz-checksum-crc32";
const CRC_32_C_HEADER_NAME: &str = "x-amz-checksum-crc32c";
const SHA_1_HEADER_NAME: &str = "x-amz-checksum-sha1";
const SHA_256_HEADER_NAME: &str = "x-amz-checksum-sha256";

const WITH_OPTIONAL_WHITESPACE: bool = true;
const TRAILER_SEPARATOR: &str = if WITH_OPTIONAL_WHITESPACE { ": " } else { ":" };

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Default)]
struct Crc32Callback {
    hasher: crc32fast::Hasher,
}

impl Crc32Callback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.update(bytes);

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    // Size of the checksum in bytes
    fn size() -> usize {
        4
    }

    fn header_name() -> HeaderName {
        HeaderName::from_static(CRC_32_HEADER_NAME)
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("base64 will always produce valid header values from checksums")
    }
}

#[derive(Debug, Default)]
struct Crc32cCallback {
    state: Option<u32>,
}

impl Crc32cCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.state = match self.state {
            Some(crc) => Some(crc32c::crc32c_append(crc, bytes)),
            None => Some(crc32c::crc32c(bytes)),
        };

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    // Size of the checksum in bytes
    fn size() -> usize {
        4
    }

    fn header_name() -> HeaderName {
        HeaderName::from_static(CRC_32_C_HEADER_NAME)
    }

    fn header_value(&self) -> HeaderValue {
        // If no data was provided to this callback and no CRC was ever calculated, return zero as the checksum.
        let hash = self.state.unwrap_or_default();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("base64 will always produce valid header values from checksums")
    }
}

#[derive(Debug, Default)]
struct Sha1Callback {
    hasher: sha1::Sha1,
}

impl Sha1Callback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    // Size of the checksum in bytes
    fn size() -> usize {
        20
    }

    fn header_name() -> HeaderName {
        HeaderName::from_static(SHA_1_HEADER_NAME)
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("base64 will always produce valid header values from checksums")
    }
}

#[derive(Debug, Default)]
struct Sha256Callback {
    hasher: sha2::Sha256,
}

impl Sha256Callback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    // Size of the checksum in bytes
    fn size() -> usize {
        32
    }

    fn header_name() -> HeaderName {
        HeaderName::from_static(SHA_256_HEADER_NAME)
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("base64 will always produce valid header values from checksums")
    }
}

enum Inner {
    Crc32(Crc32Callback),
    Crc32c(Crc32cCallback),
    Sha1(Sha1Callback),
    Sha256(Sha256Callback),
}

pub struct ChecksumCallback(Inner);

impl ChecksumCallback {
    pub fn new(checksum_algorithm: &str) -> Self {
        if checksum_algorithm.eq_ignore_ascii_case(CRC_32_NAME) {
            Self(Inner::Crc32(Crc32Callback::default()))
        } else if checksum_algorithm.eq_ignore_ascii_case(CRC_32_C_NAME) {
            Self(Inner::Crc32c(Crc32cCallback::default()))
        } else if checksum_algorithm.eq_ignore_ascii_case(SHA_1_NAME) {
            Self(Inner::Sha1(Sha1Callback::default()))
        } else if checksum_algorithm.eq_ignore_ascii_case(SHA_256_NAME) {
            Self(Inner::Sha256(Sha256Callback::default()))
        } else {
            panic!("unsupported checksum algorithm '{}'", checksum_algorithm)
        }
    }

    pub fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        match &mut self.0 {
            Inner::Crc32(ref mut callback) => callback.update(bytes)?,
            Inner::Crc32c(ref mut callback) => callback.update(bytes)?,
            Inner::Sha1(ref mut callback) => callback.update(bytes)?,
            Inner::Sha256(ref mut callback) => callback.update(bytes)?,
        };

        Ok(())
    }

    pub fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        match &self.0 {
            Inner::Sha256(callback) => callback.trailers(),
            Inner::Crc32c(callback) => callback.trailers(),
            Inner::Crc32(callback) => callback.trailers(),
            Inner::Sha1(callback) => callback.trailers(),
        }
    }

    pub fn size_hint(&self) -> SizeHint {
        let (trailer_name_size_in_bytes, checksum_size_in_bytes) = match &self.0 {
            // We want to get the size of the actual `HeaderName` except those don't have a `.len()`
            // method so we'd have to convert back to the original string. That's why we're getting
            // `.len()` from the original strings here.
            Inner::Crc32(_) => (CRC_32_HEADER_NAME.len(), Crc32Callback::size()),
            Inner::Crc32c(_) => (CRC_32_C_HEADER_NAME.len(), Crc32cCallback::size()),
            Inner::Sha1(_) => (SHA_1_HEADER_NAME.len(), Sha1Callback::size()),
            Inner::Sha256(_) => (SHA_256_HEADER_NAME.len(), Sha256Callback::size()),
        };
        // The checksums will be base64 encoded so we need to get that length
        let base64_encoded_checksum_size_in_bytes = base64::encoded_length(checksum_size_in_bytes);

        let total_size = trailer_name_size_in_bytes
            + TRAILER_SEPARATOR.len()
            + base64_encoded_checksum_size_in_bytes;

        SizeHint::with_exact(total_size as u64)
    }
}
```

```rust
// In aws-smithy-checksums/src/body.rs
use crate::ChecksumCallback;

use aws_smithy_http::body::SdkBody;
use aws_smithy_http::header::append_merge_header_maps;

use bytes::{Buf, Bytes};
use http::{HeaderMap, HeaderValue};
use http_body::{Body, SizeHint};
use pin_project::pin_project;

use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project]
pub struct ChecksumBody<InnerBody> {
    #[pin]
    inner: InnerBody,
    #[pin]
    checksum_callback: ChecksumCallback,
}

impl ChecksumBody<SdkBody> {
    pub fn new(checksum_algorithm: &str, body: SdkBody) -> Self {
        Self {
            checksum_callback: ChecksumCallback::new(checksum_algorithm),
            inner: body,
        }
    }

    fn poll_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, aws_smithy_http::body::Error>>> {
        let this = self.project();
        let inner = this.inner;
        let mut checksum_callback = this.checksum_callback;

        match inner.poll_data(cx) {
            Poll::Ready(Some(Ok(mut data))) => {
                let len = data.chunk().len();
                let bytes = data.copy_to_bytes(len);

                if let Err(e) = checksum_callback.update(&bytes) {
                    return Poll::Ready(Some(Err(e)));
                }

                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl http_body::Body for ChecksumBody<SdkBody> {
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.poll_inner(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        let this = self.project();
        match (
            this.checksum_callback.trailers(),
            http_body::Body::poll_trailers(this.inner, cx),
        ) {
            // If everything is ready, return trailers, merging them if we have more than one map
            (Ok(outer_trailers), Poll::Ready(Ok(inner_trailers))) => {
                let trailers = match (outer_trailers, inner_trailers) {
                    // Values from the inner trailer map take precedent over values from the outer map
                    (Some(outer), Some(inner)) => Some(append_merge_header_maps(inner, outer)),
                    // If only one or neither produced trailers, just combine the `Option`s with `or`
                    (outer, inner) => outer.or(inner),
                };
                Poll::Ready(Ok(trailers))
            }
            // If the inner poll is Ok but the outer body's checksum callback encountered an error,
            // return the error
            (Err(e), Poll::Ready(Ok(_))) => Poll::Ready(Err(e)),
            // Otherwise return the result of the inner poll.
            // It may be pending or it may be ready with an error.
            (_, inner_poll) => inner_poll,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        let body_size_hint = self.inner.size_hint();
        match body_size_hint.exact() {
            Some(size) => {
                let checksum_size_hint = self
                    .checksum_callback
                    .size_hint()
                    .exact()
                    .expect("checksum size is always known");
                SizeHint::with_exact(size + checksum_size_hint)
            }
            // TODO is this the right behavior?
            None => {
                let checksum_size_hint = self
                    .checksum_callback
                    .size_hint()
                    .exact()
                    .expect("checksum size is always known");
                let mut summed_size_hint = SizeHint::new();
                summed_size_hint.set_lower(body_size_hint.lower() + checksum_size_hint);

                if let Some(body_size_hint_upper) = body_size_hint.upper() {
                    summed_size_hint.set_upper(body_size_hint_upper + checksum_size_hint);
                }

                summed_size_hint
            }
        }
    }
}
```

[RFC0013]: ./rfc0013_body_callback_apis.md
[ByteStream impls]: https://github.com/awslabs/smithy-rs/blob/f76bc159bf16510a0873f5fba691cb05816f4192/rust-runtime/aws-smithy-http/src/byte_stream.rs#L205
[SdkBody impls]: https://github.com/awslabs/smithy-rs/blob/f76bc159bf16510a0873f5fba691cb05816f4192/rust-runtime/aws-smithy-http/src/body.rs#L71
