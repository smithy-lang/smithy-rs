/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functionality for validating an HTTP body against a given precalculated checksum and emitting an
//! error if it doesn't match.

use crate::http::HttpChecksum;
use crate::ChecksumAlgorithm;

use aws_smithy_types::body::SdkBody;

use bytes::Bytes;
use pin_project_lite::pin_project;

use std::fmt::Display;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

/// Whether a response body was validated against a checksum, and with which algorithm.
///
/// A checksum *mismatch* is not represented here: a mismatch surfaces as an error while reading
/// the body. This type distinguishes a body that was validated and matched from one that was not
/// validated, and records why validation did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationOutcome {
    /// The response body was validated against a checksum and matched.
    Validated {
        /// The algorithm used to validate the response body.
        algorithm: ChecksumAlgorithm,
    },
    /// The response body was not validated.
    NotValidated {
        /// Why validation did not happen.
        reason: NotValidatedReason,
    },
}

/// The reason response checksum validation did not occur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotValidatedReason {
    /// The response carried no checksum header for any supported algorithm.
    NoChecksum,
    /// The response checksum was a part-level (composite, `<base64>-<N>`) checksum, which the
    /// SDK cannot validate against the response bytes.
    PartLevelChecksum,
}

/// A shared, late-populated handle to a response's [`ValidationOutcome`].
///
/// Response checksum validation runs lazily as the response body is consumed, after the operation
/// has already returned its output. This handle is installed before body consumption and filled
/// when the body reaches end-of-stream, so a caller holding it can learn the outcome once the body
/// has been fully read. Until then, [`outcome`](Self::outcome) returns `None`.
///
/// The handle is cheap to clone (it is reference-counted) and is the value stored on an operation
/// output's extensions.
#[derive(Clone, Debug, Default)]
pub struct ResponseChecksumValidationResult(Arc<OnceLock<ValidationOutcome>>);

impl ResponseChecksumValidationResult {
    /// Create a new, unpopulated handle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the validation outcome, or `None` if the response body has not yet been fully
    /// consumed (validation is only resolved at end-of-stream).
    pub fn outcome(&self) -> Option<ValidationOutcome> {
        self.0.get().cloned()
    }

    /// Record the validation outcome. The first write wins; subsequent writes are ignored (the
    /// outcome for a given response is resolved exactly once).
    pub fn set(&self, outcome: ValidationOutcome) {
        let _ = self.0.set(outcome);
    }
}

pin_project! {
    /// A body-wrapper that will calculate the `InnerBody`'s checksum and emit an error if it
    /// doesn't match the precalculated checksum.
    pub struct ChecksumBody<InnerBody> {
        #[pin]
        inner: InnerBody,
        checksum: Option<Box<dyn HttpChecksum>>,
        precalculated_checksum: Bytes,
        // When present, the validation outcome is recorded into this handle on clean EOF.
        validation_result: Option<(ResponseChecksumValidationResult, ChecksumAlgorithm)>,
    }
}

impl ChecksumBody<SdkBody> {
    /// Given an `SdkBody`, a `Box<dyn HttpChecksum>`, and a precalculated checksum represented
    /// as `Bytes`, create a new `ChecksumBody<SdkBody>`.
    pub fn new(
        body: SdkBody,
        checksum: Box<dyn HttpChecksum>,
        precalculated_checksum: Bytes,
    ) -> Self {
        Self {
            inner: body,
            checksum: Some(checksum),
            precalculated_checksum,
            validation_result: None,
        }
    }

    /// Attach a [`ResponseChecksumValidationResult`] handle that will be populated with
    /// `Validated { algorithm }` when the body is fully read and the checksum matches. On a
    /// mismatch the handle is left unset (the mismatch surfaces as a body error instead).
    pub fn with_validation_result(
        mut self,
        result: ResponseChecksumValidationResult,
        algorithm: ChecksumAlgorithm,
    ) -> Self {
        self.validation_result = Some((result, algorithm));
        self
    }

    fn poll_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Bytes>, aws_smithy_types::body::Error>>> {
        use http_body_1x::Body;

        let this = self.project();
        let checksum = this.checksum;

        match this.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                let data = frame.data_ref().expect("Data frame should have data");
                tracing::trace!(
                    "reading {} bytes from the body and updating the checksum calculation",
                    data.len()
                );
                let checksum = match checksum.as_mut() {
                    Some(checksum) => checksum,
                    None => {
                        unreachable!("The checksum must exist because it's only taken out once the inner body has been completely polled.");
                    }
                };

                checksum.update(data);
                Poll::Ready(Some(Ok(frame)))
            }
            // Once the inner body has stopped returning data, check the checksum
            // and return an error if it doesn't match.
            Poll::Ready(None) => {
                tracing::trace!("finished reading from body, calculating final checksum");
                let checksum = match checksum.take() {
                    Some(checksum) => checksum,
                    None => {
                        // If the checksum was already taken and this was polled again anyways,
                        // then return nothing
                        return Poll::Ready(None);
                    }
                };

                let actual_checksum = checksum.finalize();
                if *this.precalculated_checksum == actual_checksum {
                    // Record a positive validation outcome (if a handle is attached) now that the
                    // whole body has been read and matched.
                    if let Some((result, algorithm)) = this.validation_result.take() {
                        result.set(ValidationOutcome::Validated { algorithm });
                    }
                    Poll::Ready(None)
                } else {
                    // So many parens it's starting to look like LISP
                    Poll::Ready(Some(Err(Box::new(Error::ChecksumMismatch {
                        expected: this.precalculated_checksum.clone(),
                        actual: actual_checksum,
                    }))))
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

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Error::ChecksumMismatch { expected, actual } => write!(
                f,
                "body checksum mismatch. expected body checksum to be {} but it was {}",
                hex::encode(expected),
                hex::encode(actual)
            ),
        }
    }
}

impl std::error::Error for Error {}

impl http_body_1x::Body for ChecksumBody<SdkBody> {
    type Data = Bytes;
    type Error = aws_smithy_types::body::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
        self.poll_inner(cx)
    }
}

#[cfg(test)]
mod tests {
    use crate::body::validate::{ChecksumBody, Error};
    use crate::ChecksumAlgorithm;
    use aws_smithy_types::body::SdkBody;
    use bytes::{Buf, Bytes};
    use bytes_utils::SegmentedBuf;
    use http_body_util::BodyExt;
    use std::io::Read;

    fn calculate_crc32_checksum(input: &str) -> Bytes {
        let checksum =
            crc_fast::checksum(crc_fast::CrcAlgorithm::Crc32IsoHdlc, input.as_bytes()) as u32;

        Bytes::copy_from_slice(&checksum.to_be_bytes())
    }

    #[tokio::test]
    async fn test_checksum_validated_body_errors_on_mismatch() {
        let input_text = "This is some test text for an SdkBody";
        let actual_checksum = calculate_crc32_checksum(input_text);
        let body = SdkBody::from(input_text);
        let non_matching_checksum = Bytes::copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let mut body = ChecksumBody::new(
            body,
            "crc32".parse::<ChecksumAlgorithm>().unwrap().into_impl(),
            non_matching_checksum.clone(),
        );

        while let Some(data) = body.frame().await {
            match data {
                Ok(_) => { /* Do nothing */ }
                Err(e) => {
                    match e.downcast_ref::<Error>().unwrap() {
                        Error::ChecksumMismatch { expected, actual } => {
                            assert_eq!(expected, &non_matching_checksum);
                            assert_eq!(actual, &actual_checksum);
                        }
                    }

                    return;
                }
            }
        }

        panic!("didn't hit expected error condition");
    }

    #[tokio::test]
    async fn test_checksum_validated_body_succeeds_on_match() {
        let input_text = "This is some test text for an SdkBody";
        let actual_checksum = calculate_crc32_checksum(input_text);
        let body = SdkBody::from(input_text);
        let http_checksum = "crc32".parse::<ChecksumAlgorithm>().unwrap().into_impl();
        let mut body = ChecksumBody::new(body, http_checksum, actual_checksum);

        let mut output = SegmentedBuf::new();
        while let Some(buf) = body.frame().await {
            let data = buf.unwrap().into_data().unwrap();
            output.push(data);
        }

        let mut output_text = String::new();
        output
            .reader()
            .read_to_string(&mut output_text)
            .expect("Doesn't cause IO errors");
        // Verify data is complete and unaltered
        assert_eq!(input_text, output_text);
    }

    #[tokio::test]
    async fn test_validation_result_populated_on_match() {
        use crate::body::validate::{ResponseChecksumValidationResult, ValidationOutcome};

        let input_text = "This is some test text for an SdkBody";
        let actual_checksum = calculate_crc32_checksum(input_text);
        let http_checksum = "crc32".parse::<ChecksumAlgorithm>().unwrap().into_impl();
        let result = ResponseChecksumValidationResult::new();
        let mut body = ChecksumBody::new(SdkBody::from(input_text), http_checksum, actual_checksum)
            .with_validation_result(result.clone(), ChecksumAlgorithm::Crc32);

        // Outcome is unresolved until the body is fully drained.
        assert_eq!(result.outcome(), None);

        while let Some(buf) = body.frame().await {
            let _ = buf.unwrap();
        }

        assert_eq!(
            result.outcome(),
            Some(ValidationOutcome::Validated {
                algorithm: ChecksumAlgorithm::Crc32
            })
        );
    }

    #[tokio::test]
    async fn test_validation_result_unset_on_mismatch() {
        use crate::body::validate::ResponseChecksumValidationResult;

        let input_text = "This is some test text for an SdkBody";
        let non_matching_checksum = Bytes::copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let http_checksum = "crc32".parse::<ChecksumAlgorithm>().unwrap().into_impl();
        let result = ResponseChecksumValidationResult::new();
        let mut body = ChecksumBody::new(
            SdkBody::from(input_text),
            http_checksum,
            non_matching_checksum,
        )
        .with_validation_result(result.clone(), ChecksumAlgorithm::Crc32);

        // Drain until the mismatch error surfaces.
        while let Some(data) = body.frame().await {
            if data.is_err() {
                break;
            }
        }

        // A mismatch leaves the outcome unset (it surfaces as a body error, not a verdict).
        assert_eq!(result.outcome(), None);
    }
}
