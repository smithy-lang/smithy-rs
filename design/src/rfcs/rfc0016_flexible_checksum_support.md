RFC: Supporting Flexible Checksums
==================================

> Status: Implemented

We can't currently update the S3 SDK because we don't support the new "Flexible Checksums" feature. This RFC describes this new feature and details how we should implement it in `smithy-rs`.

## What is the "Flexible Checksums" feature?

S3 has previously supported MD5 checksum validation of data. Now, it supports more checksum algorithms like CRC32, CRC32C, SHA-1, and SHA-256. This validation is available when putting objects to S3 and when getting them from S3. For more information, see [this AWS News Blog post][1].

## Implementing Checksums

Checksum callbacks were introduced as a result of the acceptance of [RFC0013] and this RFC proposes a refactor to those callbacks, as well as several new wrappers for `SdkBody` that will provide new functionality.

### Refactoring aws-smithy-checksums

TLDR; This refactor of aws-smithy-checksums:
- **Removes the "callback" terminology:** As a word, "callback" doesn't carry any useful information, and doesn't aid in understanding.
- **Removes support for the `BodyCallback` API:** Instead of adding checksum callbacks to a body, we're going to use a "body wrapping" instead. "Body wrapping" is demonstrated in the [`ChecksumBody`](#checksumbody), [`AwsChunkedBody`](#awschunkedbody-and-awschunkedbodyoptions), and [`ChecksumValidatedBody`](#checksumvalidatedbody) sections.

  *NOTE: This doesn't remove the `BodyCallback` trait. That will still exist, we just won't use it.*
- **Updates terminology to focus on "headers" instead of "trailers":** Because the types we deal with in this module are named for HTTP headers, I chose to use that terminology instead. My hope is that this will be less strange to people reading this code.
- **Adds `fn checksum_algorithm_to_checksum_header_name`:** a function that's used in generated code to set a checksum request header.
- **Adds `fn checksum_header_name_to_checksum_algorithm`:** a function that's used in generated code when creating a checksum-validating response body.
- **Add new checksum-related "body wrapping" HTTP body types**: These are defined in the `body` module and will be shown later in this RFC.

```rust,ignore
// In aws-smithy-checksums/src/lib.rs
//! Checksum calculation and verification callbacks

use aws_smithy_types::base64;

use bytes::Bytes;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use sha1::Digest;
use std::io::Write;

pub mod body;

// Valid checksum algorithm names
pub const CRC_32_NAME: &str = "crc32";
pub const CRC_32_C_NAME: &str = "crc32c";
pub const SHA_1_NAME: &str = "sha1";
pub const SHA_256_NAME: &str = "sha256";

pub const CRC_32_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-crc32");
pub const CRC_32_C_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-crc32c");
pub const SHA_1_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-sha1");
pub const SHA_256_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-sha256");

// Preserved for compatibility purposes. This should never be used by users, only within smithy-rs
const MD5_NAME: &str = "md5";
const MD5_HEADER_NAME: HeaderName = HeaderName::from_static("content-md5");

/// Given a `&str` representing a checksum algorithm, return the corresponding `HeaderName`
/// for that checksum algorithm.
pub fn checksum_algorithm_to_checksum_header_name(checksum_algorithm: &str) -> HeaderName {
    if checksum_algorithm.eq_ignore_ascii_case(CRC_32_NAME) {
        CRC_32_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(CRC_32_C_NAME) {
        CRC_32_C_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_1_NAME) {
        SHA_1_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_256_NAME) {
        SHA_256_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(MD5_NAME) {
        MD5_HEADER_NAME
    } else {
        // TODO what's the best way to handle this case?
        HeaderName::from_static("x-amz-checksum-unknown")
    }
}

/// Given a `HeaderName` representing a checksum algorithm, return the name of that algorithm
/// as a `&'static str`.
pub fn checksum_header_name_to_checksum_algorithm(
    checksum_header_name: &HeaderName,
) -> &'static str {
    if checksum_header_name == CRC_32_HEADER_NAME {
        CRC_32_NAME
    } else if checksum_header_name == CRC_32_C_HEADER_NAME {
        CRC_32_C_NAME
    } else if checksum_header_name == SHA_1_HEADER_NAME {
        SHA_1_NAME
    } else if checksum_header_name == SHA_256_HEADER_NAME {
        SHA_256_NAME
    } else if checksum_header_name == MD5_HEADER_NAME {
        MD5_NAME
    } else {
        // TODO what's the best way to handle this case?
        "unknown-checksum-algorithm"
    }
}

/// When a response has to be checksum-verified, we have to check possible headers until we find the
/// header with the precalculated checksum. Because a service may send back multiple headers, we have
/// to check them in order based on how fast each checksum is to calculate.
pub const CHECKSUM_HEADERS_IN_PRIORITY_ORDER: [HeaderName; 4] = [
    CRC_32_C_HEADER_NAME,
    CRC_32_HEADER_NAME,
    SHA_1_HEADER_NAME,
    SHA_256_HEADER_NAME,
];

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Checksum algorithms are use to validate the integrity of data. Structs that implement this trait
/// can be used as checksum calculators. This trait requires Send + Sync because these checksums are
/// often used in a threaded context.
pub trait Checksum: Send + Sync {
    /// Given a slice of bytes, update this checksum's internal state.
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError>;
    /// Either return this checksum as a `HeaderMap` containing one HTTP header, or return an error
    /// describing why checksum calculation failed.
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError>;
    /// Return the `HeaderName` used to represent this checksum algorithm
    fn header_name(&self) -> HeaderName;
    /// "Finalize" this checksum, returning the calculated value as `Bytes` or an error that
    /// occurred during checksum calculation. To print this value in a human-readable hexadecimal
    /// format, you can print it using Rust's builtin [formatter].
    ///
    /// _**NOTE:** typically, "finalizing" a checksum in Rust will take ownership of the checksum
    /// struct. In this method, we clone the checksum's state before finalizing because checksums
    /// may be used in a situation where taking ownership is not possible._
    ///
    /// [formatter]: https://doc.rust-lang.org/std/fmt/trait.UpperHex.html
    fn finalize(&self) -> Result<Bytes, BoxError>;
    /// Return the size of this checksum algorithms resulting checksum, in bytes. For example, the
    /// CRC32 checksum algorithm calculates a 32 bit checksum, so a CRC32 checksum struct
    /// implementing this trait method would return 4.
    fn size(&self) -> u64;
}

/// Create a new `Box<dyn Checksum>` from an algorithm name. Valid algorithm names are defined as
/// `const`s in this module.
pub fn new_checksum(checksum_algorithm: &str) -> Box<dyn Checksum> {
    if checksum_algorithm.eq_ignore_ascii_case(CRC_32_NAME) {
        Box::new(Crc32::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(CRC_32_C_NAME) {
        Box::new(Crc32c::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_1_NAME) {
        Box::new(Sha1::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_256_NAME) {
        Box::new(Sha256::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(MD5_NAME) {
        // It's possible to create an MD5 and we do this in some situations for compatibility.
        // We deliberately hide this from users so that they don't go using it.
        Box::new(Md5::default())
    } else {
        panic!("unsupported checksum algorithm '{}'", checksum_algorithm)
    }
}

#[derive(Debug, Default)]
struct Crc32 {
    hasher: crc32fast::Hasher,
}

impl Crc32 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.update(bytes);

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            &self.hasher.clone().finalize().to_be_bytes(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        4
    }

    fn header_name() -> HeaderName {
        CRC_32_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("will always produce a valid header value from a CRC32 checksum")
    }
}

impl Checksum for Crc32 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Crc32c {
    state: Option<u32>,
}

impl Crc32c {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.state = match self.state {
            Some(crc) => Some(crc32c::crc32c_append(crc, bytes)),
            None => Some(crc32c::crc32c(bytes)),
        };

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            &self.state.unwrap_or_default().to_be_bytes(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        4
    }

    fn header_name() -> HeaderName {
        CRC_32_C_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // If no data was provided to this callback and no CRC was ever calculated, return zero as the checksum.
        let hash = self.state.unwrap_or_default();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("will always produce a valid header value from a CRC32C checksum")
    }
}

impl Checksum for Crc32c {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Sha1 {
    hasher: sha1::Sha1,
}

impl Sha1 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        20
    }

    fn header_name() -> HeaderName {
        SHA_1_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from a SHA-1 checksum")
    }
}

impl Checksum for Sha1 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Sha256 {
    hasher: sha2::Sha256,
}

impl Sha256 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        32
    }

    fn header_name() -> HeaderName {
        SHA_256_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from a SHA-256 checksum")
    }
}

impl Checksum for Sha256 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Md5 {
    hasher: md5::Md5,
}

impl Md5 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        16
    }

    fn header_name() -> HeaderName {
        MD5_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from an MD5 checksum")
    }
}

impl Checksum for Md5 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

// We have existing tests for the checksums, those don't require an update
```

### `ChecksumBody`

When creating a checksum-validated request with an in-memory request body, we can read the body, calculate a checksum, and insert the checksum header, all before sending the request. When creating a checksum-validated request with a streaming request body, we don't have that luxury. Instead, we must calculate a checksum while sending the body, and append that checksum as a [trailer][2].

We will accomplish this by wrapping the `SdkBody` that requires validation within a `ChecksumBody`. Afterwards, we'll need to wrap the `ChecksumBody` in yet another layer which we'll discuss in the [`AwsChunkedBody` and `AwsChunkedBodyOptions`](#awschunkedbody-and-awschunkedbodyoptions) section.

```rust,ignore
// In aws-smithy-checksums/src/body.rs
use crate::{new_checksum, Checksum};

use aws_smithy_http::body::SdkBody;
use aws_smithy_http::header::append_merge_header_maps;
use aws_smithy_types::base64;

use bytes::{Buf, Bytes};
use http::header::HeaderName;
use http::{HeaderMap, HeaderValue};
use http_body::{Body, SizeHint};
use pin_project::pin_project;

use std::fmt::Display;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A `ChecksumBody` will read and calculate a request body as it's being sent. Once the body has
/// been completely read, it'll append a trailer with the calculated checksum.
#[pin_project]
pub struct ChecksumBody<InnerBody> {
    #[pin]
    inner: InnerBody,
    #[pin]
    checksum: Box<dyn Checksum>,
}

impl ChecksumBody<SdkBody> {
    /// Given an `SdkBody` and the name of a checksum algorithm as a `&str`, create a new
    /// `ChecksumBody<SdkBody>`. Valid checksum algorithm names are defined in this crate's
    /// [root module](super).
    ///
    /// # Panics
    ///
    /// This will panic if the given checksum algorithm is not supported.
    pub fn new(body: SdkBody, checksum_algorithm: &str) -> Self {
        Self {
            checksum: new_checksum(checksum_algorithm),
            inner: body,
        }
    }

    /// Return the name of the trailer that will be emitted by this `ChecksumBody`
    pub fn trailer_name(&self) -> HeaderName {
        self.checksum.header_name()
    }

    /// Calculate and return the sum of the:
    /// - checksum when base64 encoded
    /// - trailer name
    /// - trailer separator
    ///
    /// This is necessary for calculating the true size of the request body for certain
    /// content-encodings.
    pub fn trailer_length(&self) -> u64 {
        let trailer_name_size_in_bytes = self.checksum.header_name().as_str().len() as u64;
        let base64_encoded_checksum_size_in_bytes = base64::encoded_length(self.checksum.size());

        (trailer_name_size_in_bytes
            // HTTP trailer names and values may be separated by either a single colon or a single
            // colon and a whitespace. In the AWS Rust SDK, we use a single colon.
            + ":".len() as u64
            + base64_encoded_checksum_size_in_bytes)
    }

    fn poll_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, aws_smithy_http::body::Error>>> {
        let this = self.project();
        let inner = this.inner;
        let mut checksum = this.checksum;

        match inner.poll_data(cx) {
            Poll::Ready(Some(Ok(mut data))) => {
                let len = data.chunk().len();
                let bytes = data.copy_to_bytes(len);

                if let Err(e) = checksum.update(&bytes) {
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
            this.checksum.headers(),
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
                let checksum_size_hint = self.checksum.size();
                SizeHint::with_exact(size + checksum_size_hint)
            }
            // TODO is this the right behavior?
            None => {
                let checksum_size_hint = self.checksum.size();
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

// The tests I have written are omitted from this RFC for brevity. The request body checksum calculation and trailer size calculations are all tested.
```

### `ChecksumValidatedBody`

Users may request checksum validation for response bodies. That capability is provided by `ChecksumValidatedBody`, which will calculate a checksum as the response body is being read. Once all data has been read, the calculated checksum is compared to a precalculated checksum set during body creation. If the checksums don't match, then the body will emit an error.

```rust,ignore
// In aws-smithy-checksums/src/body.rs
/// A response body that will calculate a checksum as it is read. If all data is read and the
/// calculated checksum doesn't match a precalculated checksum, this body will emit an
/// [asw_smithy_http::body::Error].
#[pin_project]
pub struct ChecksumValidatedBody<InnerBody> {
    #[pin]
    inner: InnerBody,
    #[pin]
    checksum: Box<dyn Checksum>,
    precalculated_checksum: Bytes,
}

impl ChecksumValidatedBody<SdkBody> {
    /// Given an `SdkBody`, the name of a checksum algorithm as a `&str`, and a precalculated
    /// checksum represented as `Bytes`, create a new `ChecksumValidatedBody<SdkBody>`.
    /// Valid checksum algorithm names are defined in this crate's [root module](super).
    ///
    /// # Panics
    ///
    /// This will panic if the given checksum algorithm is not supported.
    pub fn new(body: SdkBody, checksum_algorithm: &str, precalculated_checksum: Bytes) -> Self {
        Self {
            checksum: new_checksum(checksum_algorithm),
            inner: body,
            precalculated_checksum,
        }
    }

    fn poll_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, aws_smithy_http::body::Error>>> {
        let this = self.project();
        let inner = this.inner;
        let mut checksum = this.checksum;

        match inner.poll_data(cx) {
            Poll::Ready(Some(Ok(mut data))) => {
                let len = data.chunk().len();
                let bytes = data.copy_to_bytes(len);

                if let Err(e) = checksum.update(&bytes) {
                    return Poll::Ready(Some(Err(e)));
                }

                Poll::Ready(Some(Ok(bytes)))
            }
            // Once the inner body has stopped returning data, check the checksum
            // and return an error if it doesn't match.
            Poll::Ready(None) => {
                let actual_checksum = {
                    match checksum.finalize() {
                        Ok(checksum) => checksum,
                        Err(err) => {
                            return Poll::Ready(Some(Err(err)));
                        }
                    }
                };
                if *this.precalculated_checksum == actual_checksum {
                    Poll::Ready(None)
                } else {
                    // So many parens it's starting to look like LISP
                    Poll::Ready(Some(Err(Box::new(Error::checksum_mismatch(
                        this.precalculated_checksum.clone(),
                        actual_checksum,
                    )))))
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Errors related to checksum calculation and validation
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// The actual checksum didn't match the expected checksum. The checksummed data has been
    /// altered since the expected checksum was calculated.
    ChecksumMismatch { expected: Bytes, actual: Bytes },
}

impl Error {
    /// Given an expected checksum and an actual checksum in `Bytes` form, create a new
    /// `Error::ChecksumMismatch`.
    pub fn checksum_mismatch(expected: Bytes, actual: Bytes) -> Self {
        Self::ChecksumMismatch { expected, actual }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Error::ChecksumMismatch { expected, actual } => write!(
                f,
                "body checksum mismatch. expected body checksum to be {:x} but it was {:x}",
                expected, actual
            ),
        }
    }
}

impl std::error::Error for Error {}

impl http_body::Body for ChecksumValidatedBody<SdkBody> {
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
        self.project().inner.poll_trailers(cx)
    }

    // Once the inner body returns true for is_end_stream, we still need to
    // verify the checksum; Therefore, we always return false here.
    fn is_end_stream(&self) -> bool {
        false
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

// The tests I have written are omitted from this RFC for brevity. The response body checksum verification is tested.
```

### `AwsChunkedBody` and `AwsChunkedBodyOptions`

In order to send a request with checksum trailers, we must use an AWS-specific content encoding called `aws-chunked`. This encoding requires that we:
- Divide the original body content into one or more chunks. For our purposes we only ever use one chunk.
- Append a hexadecimal chunk size header to each chunk.
- Suffix each chunk with a [CRLF (carriage return line feed)][3].
- Send a 0 and CRLF to close the original body content section.
- Send trailers as part of the request body, suffixing each with a CRLF.
- Send a final CRLF to close the request body.

As an example, Sending a regular request body with a SHA-256 checksum would look similar to this:

```HTTP
PUT SOMEURL HTTP/1.1
x-amz-checksum-sha256: ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=
Content-Length: 11
...

Hello world
```

and the `aws-chunked` version would look like this:

```HTTP
PUT SOMEURL HTTP/1.1
x-amz-trailer: x-amz-checksum-sha256
x-amz-decoded-content-length: 11
Content-Encoding: aws-chunked
Content-Length: 87
...

B\r\n
Hello world\r\n
0\r\n
x-amz-checksum-sha256:ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=\r\n
\r\n
```

_**NOTES:**_
- *In the second example, `B` is the hexadecimal representation of 11.*
- *Authorization and other headers are omitted from the examples above for brevity.*
- *When using `aws-chunked` content encoding, S3 requires that we send the `x-amz-decoded-content-length` with the length of the original body content.*

This encoding scheme is performed by `AwsChunkedBody` and configured with `AwsChunkedBodyOptions`.

```rust,ignore
// In aws-http/src/content_encoding.rs
use aws_smithy_checksums::body::ChecksumBody;
use aws_smithy_http::body::SdkBody;

use bytes::{Buf, Bytes, BytesMut};
use http::{HeaderMap, HeaderValue};
use http_body::{Body, SizeHint};
use pin_project::pin_project;

use std::pin::Pin;
use std::task::{Context, Poll};

const CRLF: &str = "\r\n";
const CHUNK_TERMINATOR: &str = "0\r\n";

/// Content encoding header value constants
pub mod header_value {
    /// Header value denoting "aws-chunked" encoding
    pub const AWS_CHUNKED: &str = "aws-chunked";
}

/// Options used when constructing an [`AwsChunkedBody`][AwsChunkedBody].
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct AwsChunkedBodyOptions {
    /// The total size of the stream. For unsigned encoding this implies that
    /// there will only be a single chunk containing the underlying payload,
    /// unless ChunkLength is also specified.
    pub stream_length: Option<u64>,
    /// The maximum size of each chunk to be sent.
    ///
    /// If ChunkLength and stream_length are both specified, the stream will be
    /// broken up into chunk_length chunks. The encoded length of the aws-chunked
    /// encoding can still be determined as long as all trailers, if any, have a
    /// fixed length.
    pub chunk_length: Option<u64>,
    /// The length of each trailer sent within an `AwsChunkedBody`. Necessary in
    /// order to correctly calculate the total size of the body accurately.
    pub trailer_lens: Vec<u64>,
}

impl AwsChunkedBodyOptions {
    /// Create a new [`AwsChunkedBodyOptions`][AwsChunkedBodyOptions]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set stream length
    pub fn with_stream_length(mut self, stream_length: u64) -> Self {
        self.stream_length = Some(stream_length);
        self
    }

    /// Set chunk length
    pub fn with_chunk_length(mut self, chunk_length: u64) -> Self {
        self.chunk_length = Some(chunk_length);
        self
    }

    /// Set a trailer len
    pub fn with_trailer_len(mut self, trailer_len: u64) -> Self {
        self.trailer_lens.push(trailer_len);
        self
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AwsChunkedBodyState {
    WritingChunkSize,
    WritingChunk,
    WritingTrailers,
    Closed,
}

/// A request body compatible with `Content-Encoding: aws-chunked`
///
/// Chunked-Body grammar is defined in [ABNF] as:
///
/// ```txt
/// Chunked-Body    = *chunk
///                   last-chunk
///                   chunked-trailer
///                   CRLF
///
/// chunk           = chunk-size CRLF chunk-data CRLF
/// chunk-size      = 1*HEXDIG
/// last-chunk      = 1*("0") CRLF
/// chunked-trailer = *( entity-header CRLF )
/// entity-header   = field-name ":" OWS field-value OWS
/// ```
/// For more info on what the abbreviations mean, see https://datatracker.ietf.org/doc/html/rfc7230#section-1.2
///
/// [ABNF]:https://en.wikipedia.org/wiki/Augmented_Backus%E2%80%93Naur_form
#[derive(Debug)]
#[pin_project]
pub struct AwsChunkedBody<InnerBody> {
    #[pin]
    inner: InnerBody,
    #[pin]
    state: AwsChunkedBodyState,
    options: AwsChunkedBodyOptions,
}

// Currently, we only use this in terms of a streaming request body with checksum trailers
type Inner = ChecksumBody<SdkBody>;

impl AwsChunkedBody<Inner> {
    /// Wrap the given body in an outer body compatible with `Content-Encoding: aws-chunked`
    pub fn new(body: Inner, options: AwsChunkedBodyOptions) -> Self {
        Self {
            inner: body,
            state: AwsChunkedBodyState::WritingChunkSize,
            options,
        }
    }

    fn encoded_length(&self) -> Option<u64> {
        if self.options.chunk_length.is_none() && self.options.stream_length.is_none() {
            return None;
        }

        let mut length = 0;
        let stream_length = self.options.stream_length.unwrap_or_default();
        if stream_length != 0 {
            if let Some(chunk_length) = self.options.chunk_length {
                let num_chunks = stream_length / chunk_length;
                length += num_chunks * get_unsigned_chunk_bytes_length(chunk_length);
                let remainder = stream_length % chunk_length;
                if remainder != 0 {
                    length += get_unsigned_chunk_bytes_length(remainder);
                }
            } else {
                length += get_unsigned_chunk_bytes_length(stream_length);
            }
        }

        // End chunk
        length += CHUNK_TERMINATOR.len() as u64;

        // Trailers
        for len in self.options.trailer_lens.iter() {
            length += len + CRLF.len() as u64;
        }

        // Encoding terminator
        length += CRLF.len() as u64;

        Some(length)
    }
}

fn prefix_with_chunk_size(data: Bytes, chunk_size: u64) -> Bytes {
    // Len is the size of the entire chunk as defined in `AwsChunkedBodyOptions`
    let mut prefixed_data = BytesMut::from(format!("{:X?}\r\n", chunk_size).as_bytes());
    prefixed_data.extend_from_slice(&data);

    prefixed_data.into()
}

fn get_unsigned_chunk_bytes_length(payload_length: u64) -> u64 {
    let hex_repr_len = int_log16(payload_length);
    hex_repr_len + CRLF.len() as u64 + payload_length + CRLF.len() as u64
}

fn trailers_as_aws_chunked_bytes(
    total_length_of_trailers_in_bytes: u64,
    trailer_map: Option<HeaderMap>,
) -> Bytes {
    use std::fmt::Write;

    // On 32-bit operating systems, we might not be able to convert the u64 to a usize, so we just
    // use `String::new` in that case.
    let mut trailers = match usize::try_from(total_length_of_trailers_in_bytes) {
        Ok(total_length_of_trailers_in_bytes) => {
            String::with_capacity(total_length_of_trailers_in_bytes)
        }
        Err(_) => String::new(),
    };
    let mut already_wrote_first_trailer = false;

    if let Some(trailer_map) = trailer_map {
        for (header_name, header_value) in trailer_map.into_iter() {
            match header_name {
                // New name, new value
                Some(header_name) => {
                    if already_wrote_first_trailer {
                        // First trailer shouldn't have a preceding CRLF, but every trailer after it should
                        trailers.write_str(CRLF).unwrap();
                    } else {
                        already_wrote_first_trailer = true;
                    }

                    trailers.write_str(header_name.as_str()).unwrap();
                    trailers.write_char(':').unwrap();
                }
                // Same name, new value
                None => {
                    trailers.write_char(',').unwrap();
                }
            }
            trailers.write_str(header_value.to_str().unwrap()).unwrap();
        }
    }

    // Write CRLF to end the body
    trailers.write_str(CRLF).unwrap();
    // If we wrote at least one trailer, we need to write an extra CRLF
    if total_length_of_trailers_in_bytes != 0 {
        trailers.write_str(CRLF).unwrap();
    }

    trailers.into()
}

impl Body for AwsChunkedBody<Inner> {
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        tracing::info!("polling AwsChunkedBody");
        let mut this = self.project();

        match *this.state {
            AwsChunkedBodyState::WritingChunkSize => match this.inner.poll_data(cx) {
                Poll::Ready(Some(Ok(data))) => {
                    // A chunk must be prefixed by chunk size in hexadecimal
                    tracing::info!("writing chunk size and start of chunk");
                    *this.state = AwsChunkedBodyState::WritingChunk;
                    let total_chunk_size = this
                        .options
                        .chunk_length
                        .or(this.options.stream_length)
                        .unwrap_or_default();
                    Poll::Ready(Some(Ok(prefix_with_chunk_size(data, total_chunk_size))))
                }
                Poll::Ready(None) => {
                    tracing::info!("chunk was empty, writing last-chunk");
                    *this.state = AwsChunkedBodyState::WritingTrailers;
                    Poll::Ready(Some(Ok(Bytes::from("0\r\n"))))
                }
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
                Poll::Pending => Poll::Pending,
            },
            AwsChunkedBodyState::WritingChunk => match this.inner.poll_data(cx) {
                Poll::Ready(Some(Ok(mut data))) => {
                    tracing::info!("writing rest of chunk data");
                    Poll::Ready(Some(Ok(data.copy_to_bytes(data.len()))))
                }
                Poll::Ready(None) => {
                    tracing::info!("no more chunk data, writing CRLF and last-chunk");
                    *this.state = AwsChunkedBodyState::WritingTrailers;
                    Poll::Ready(Some(Ok(Bytes::from("\r\n0\r\n"))))
                }
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
                Poll::Pending => Poll::Pending,
            },
            AwsChunkedBodyState::WritingTrailers => {
                return match this.inner.poll_trailers(cx) {
                    Poll::Ready(Ok(trailers)) => {
                        *this.state = AwsChunkedBodyState::Closed;
                        let total_length_of_trailers_in_bytes =
                            this.options.trailer_lens.iter().fold(0, |acc, n| acc + n);

                        Poll::Ready(Some(Ok(trailers_as_aws_chunked_bytes(
                            total_length_of_trailers_in_bytes,
                            trailers,
                        ))))
                    }
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
                };
            }
            AwsChunkedBodyState::Closed => {
                return Poll::Ready(None);
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        // Trailers were already appended to the body because of the content encoding scheme
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        self.state == AwsChunkedBodyState::Closed
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(
            self.encoded_length()
                .expect("Requests made with aws-chunked encoding must have known size")
                as u64,
        )
    }
}

// Used for finding how many hexadecimal digits it takes to represent a base 10 integer
fn int_log16<T>(mut i: T) -> u64
where
    T: std::ops::DivAssign + PartialOrd + From<u8> + Copy,
{
    let mut len = 0;
    let zero = T::from(0);
    let sixteen = T::from(16);

    while i > zero {
        i /= sixteen;
        len += 1;
    }

    len
}

#[cfg(test)]
mod tests {
    use super::AwsChunkedBody;
    use crate::content_encoding::AwsChunkedBodyOptions;
    use aws_smithy_checksums::body::ChecksumBody;
    use aws_smithy_http::body::SdkBody;
    use bytes::Buf;
    use bytes_utils::SegmentedBuf;
    use http_body::Body;
    use std::io::Read;

    #[tokio::test]
    async fn test_aws_chunked_encoded_body() {
        let input_text = "Hello world";
        let sdk_body = SdkBody::from(input_text);
        let checksum_body = ChecksumBody::new(sdk_body, "sha256");
        let aws_chunked_body_options = AwsChunkedBodyOptions {
            stream_length: Some(input_text.len() as u64),
            chunk_length: None,
            trailer_lens: vec![
                "x-amz-checksum-sha256:ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=".len() as u64,
            ],
        };
        let mut aws_chunked_body = AwsChunkedBody::new(checksum_body, aws_chunked_body_options);

        let mut output = SegmentedBuf::new();
        while let Some(buf) = aws_chunked_body.data().await {
            output.push(buf.unwrap());
        }

        let mut actual_output = String::new();
        output
            .reader()
            .read_to_string(&mut actual_output)
            .expect("Doesn't cause IO errors");

        let expected_output = "B\r\nHello world\r\n0\r\nx-amz-checksum-sha256:ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=\r\n\r\n";

        // Verify data is complete and correctly encoded
        assert_eq!(expected_output, actual_output);

        assert!(
            aws_chunked_body
                .trailers()
                .await
                .expect("checksum generation was without error")
                .is_none(),
            "aws-chunked encoded bodies don't have normal HTTP trailers"
        );
    }

    #[tokio::test]
    async fn test_empty_aws_chunked_encoded_body() {
        let sdk_body = SdkBody::from("");
        let checksum_body = ChecksumBody::new(sdk_body, "sha256");
        let aws_chunked_body_options = AwsChunkedBodyOptions {
            stream_length: Some(0),
            chunk_length: None,
            trailer_lens: vec![
                "x-amz-checksum-sha256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=".len() as u64,
            ],
        };
        let mut aws_chunked_body = AwsChunkedBody::new(checksum_body, aws_chunked_body_options);

        let mut output = SegmentedBuf::new();
        while let Some(buf) = aws_chunked_body.data().await {
            output.push(buf.unwrap());
        }

        let mut actual_output = String::new();
        output
            .reader()
            .read_to_string(&mut actual_output)
            .expect("Doesn't cause IO errors");

        let expected_output =
            "0\r\nx-amz-checksum-sha256:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=\r\n\r\n";

        // Verify data is complete and correctly encoded
        assert_eq!(expected_output, actual_output);

        assert!(
            aws_chunked_body
                .trailers()
                .await
                .expect("checksum generation was without error")
                .is_none(),
            "aws-chunked encoded bodies don't have normal HTTP trailers"
        );
    }
}
```

### Sigv4 Update

When sending checksum-verified requests with a streaming body, we must update the usual signing process. Instead of signing the request based on the request body's checksum, we must sign it with a special header instead:

```HTTP
Authorization: <computed authorization header value using "STREAMING-UNSIGNED-PAYLOAD-TRAILER">
x-amz-content-sha256: STREAMING-UNSIGNED-PAYLOAD-TRAILER
```

Setting `STREAMING-UNSIGNED-PAYLOAD-TRAILER` tells the signer that we're sending an unsigned streaming body that will be followed by trailers.

We can achieve this by:
- Adding a new variant to `SignableBody`:
  ```rust,ignore
  /// A signable HTTP request body
  #[derive(Debug, Clone, Eq, PartialEq)]
  #[non_exhaustive]
  pub enum SignableBody<'a> {
      // existing variants have been omitted for brevity...

      /// An unsigned payload with trailers
      ///
      /// StreamingUnsignedPayloadTrailer is used for streaming requests where the contents of the
      /// body cannot be known prior to signing **AND** which include HTTP trailers.
      StreamingUnsignedPayloadTrailer,
  }
  ```
- Updating the `CanonicalRequest::payload_hash` method to include the new `SignableBody` variant:
  ```rust,ignore
  fn payload_hash<'b>(body: &'b SignableBody<'b>) -> Cow<'b, str> {
      // Payload hash computation
      //
      // Based on the input body, set the payload_hash of the canonical request:
      // Either:
      // - compute a hash
      // - use the precomputed hash
      // - use `UnsignedPayload`
      // - use `StreamingUnsignedPayloadTrailer`
      match body {
          SignableBody::Bytes(data) => Cow::Owned(sha256_hex_string(data)),
          SignableBody::Precomputed(digest) => Cow::Borrowed(digest.as_str()),
          SignableBody::UnsignedPayload => Cow::Borrowed(UNSIGNED_PAYLOAD),
          SignableBody::StreamingUnsignedPayloadTrailer => {
              Cow::Borrowed(STREAMING_UNSIGNED_PAYLOAD_TRAILER)
          }
      }
  }
  ```
- *(in generated code)* Inserting the `SignableBody` into the request property bag when making a checksum-verified streaming request:
  ```rust,ignore
  if self.checksum_algorithm.is_some() {
      request
          .properties_mut()
          .insert(aws_sig_auth::signer::SignableBody::StreamingUnsignedPayloadTrailer);
  }
  ```

It's possible to send `aws-chunked` requests where each chunk is signed individually. Because this feature isn't strictly necessary for flexible checksums, I've avoided implementing it.

### Inlineables

In order to avoid writing lots of Rust in Kotlin, I have implemented request and response building functions as inlineables:

- Building checksum-validated requests with in-memory request bodies:
  ```rust,ignore
  // In aws/rust-runtime/aws-inlineable/src/streaming_body_with_checksum.rs
  /// Given a `&mut http::request::Request`, and checksum algorithm name, calculate a checksum and
  /// then modify the request to include the checksum as a header.
  pub fn build_checksum_validated_request(
      request: &mut http::request::Request<aws_smithy_http::body::SdkBody>,
      checksum_algorithm: &str,
  ) -> Result<(), aws_smithy_http::operation::BuildError> {
      let data = request.body().bytes().unwrap_or_default();

      let mut checksum = aws_smithy_checksums::new_checksum(checksum_algorithm);
      checksum
          .update(data)
          .map_err(|err| aws_smithy_http::operation::BuildError::Other(err))?;
      let checksum = checksum
          .finalize()
          .map_err(|err| aws_smithy_http::operation::BuildError::Other(err))?;

      request.headers_mut().insert(
          aws_smithy_checksums::checksum_algorithm_to_checksum_header_name(checksum_algorithm),
          aws_smithy_types::base64::encode(&checksum[..])
              .parse()
              .expect("base64-encoded checksums are always valid header values"),
      );

      Ok(())
  }
  ```
- Building checksum-validated requests with streaming request bodies:
  ```rust,ignore
  /// Given an `http::request::Builder`, `SdkBody`, and a checksum algorithm name, return a
  /// `Request<SdkBody>` with checksum trailers where the content is `aws-chunked` encoded.
  pub fn build_checksum_validated_request_with_streaming_body(
      request_builder: http::request::Builder,
      body: aws_smithy_http::body::SdkBody,
      checksum_algorithm: &str,
  ) -> Result<http::Request<aws_smithy_http::body::SdkBody>, aws_smithy_http::operation::BuildError> {
      use http_body::Body;

      let original_body_size = body
          .size_hint()
          .exact()
          .expect("body must be sized if checksum is requested");
      let body = aws_smithy_checksums::body::ChecksumBody::new(body, checksum_algorithm);
      let checksum_trailer_name = body.trailer_name();
      let aws_chunked_body_options = aws_http::content_encoding::AwsChunkedBodyOptions::new()
          .with_stream_length(original_body_size as usize)
          .with_trailer_len(body.trailer_length() as usize);

      let body = aws_http::content_encoding::AwsChunkedBody::new(body, aws_chunked_body_options);
      let encoded_content_length = body
          .size_hint()
          .exact()
          .expect("encoded_length must return known size");
      let request_builder = request_builder
          .header(
              http::header::CONTENT_LENGTH,
              http::HeaderValue::from(encoded_content_length),
          )
          .header(
              http::header::HeaderName::from_static("x-amz-decoded-content-length"),
              http::HeaderValue::from(original_body_size),
          )
          .header(
              http::header::HeaderName::from_static("x-amz-trailer"),
              checksum_trailer_name,
          )
          .header(
              http::header::CONTENT_ENCODING,
              aws_http::content_encoding::header_value::AWS_CHUNKED.as_bytes(),
          );

      let body = aws_smithy_http::body::SdkBody::from_dyn(http_body::combinators::BoxBody::new(body));

      request_builder
          .body(body)
          .map_err(|err| aws_smithy_http::operation::BuildError::Other(Box::new(err)))
  }
  ```
- Building checksum-validated responses:
  ```rust,ignore
  /// Given a `Response<SdkBody>`, checksum algorithm name, and pre-calculated checksum, return a
  /// `Response<SdkBody>` where the body will processed with the checksum algorithm and checked
  /// against the pre-calculated checksum.
  pub fn build_checksum_validated_sdk_body(
      body: aws_smithy_http::body::SdkBody,
      checksum_algorithm: &str,
      precalculated_checksum: bytes::Bytes,
  ) -> aws_smithy_http::body::SdkBody {
      let body = aws_smithy_checksums::body::ChecksumValidatedBody::new(
          body,
          checksum_algorithm,
          precalculated_checksum.clone(),
      );
      aws_smithy_http::body::SdkBody::from_dyn(http_body::combinators::BoxBody::new(body))
  }

  /// Given the name of a checksum algorithm and a `HeaderMap`, extract the checksum value from the
  /// corresponding header as `Some(Bytes)`. If the header is unset, return `None`.
  pub fn check_headers_for_precalculated_checksum(
      headers: &http::HeaderMap<http::HeaderValue>,
  ) -> Option<(&'static str, bytes::Bytes)> {
      for header_name in aws_smithy_checksums::CHECKSUM_HEADERS_IN_PRIORITY_ORDER {
          if let Some(precalculated_checksum) = headers.get(&header_name) {
              let checksum_algorithm =
                  aws_smithy_checksums::checksum_header_name_to_checksum_algorithm(&header_name);
              let precalculated_checksum =
                  bytes::Bytes::copy_from_slice(precalculated_checksum.as_bytes());

              return Some((checksum_algorithm, precalculated_checksum));
          }
      }

      None
  }
  ```

## Codegen

Codegen will be updated to insert the appropriate inlineable functions for operations that are tagged with the `@httpchecksum` trait. Some operations will require an MD5 checksum fallback if the user hasn't set a checksum themselves.

Users also have the option of supplying a precalculated checksum of their own. This is already handled by our current header insertion logic and won't require updating the existing implementation. Because this checksum validation behavior is AWS-specific, it will be defined in SDK codegen.

## Implementation Checklist

- [ ] Implement codegen for building checksum-validated requests:
  - [ ] In-memory request bodies
    - [ ] Support MD5 fallback behavior for services that enable it.
  - [ ] Streaming request bodies
- [ ] Implement codegen for building checksum-validated responses:

[1]: https://aws.amazon.com/blogs/aws/new-additional-checksum-algorithms-for-amazon-s3/
[2]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Trailer
[3]: https://developer.mozilla.org/en-US/docs/Glossary/CRLF
[RFC0013]: ./rfc0013_body_callback_apis.md
