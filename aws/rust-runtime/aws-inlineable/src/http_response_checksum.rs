/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(dead_code)]

//! Interceptor for handling Smithy `@httpChecksum` response checksumming

use aws_smithy_checksums::ChecksumAlgorithm;
use aws_smithy_runtime::client::sdk_feature::SmithySdkFeature;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeDeserializationInterceptorContextMut, BeforeSerializationInterceptorContextMut, Input,
};
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::checksum_config::ResponseChecksumValidation;
use aws_smithy_types::config_bag::{ConfigBag, Layer, Storable, StoreReplace};
use std::{fmt, mem};

#[derive(Debug)]
struct ResponseChecksumInterceptorState {
    validation_enabled: bool,
}
impl Storable for ResponseChecksumInterceptorState {
    type Storer = StoreReplace<Self>;
}

pub(crate) struct ResponseChecksumInterceptor<VE, CM> {
    response_algorithms: &'static [&'static str],
    validation_enabled: VE,
    checksum_mutator: CM,
}

impl<VE, CM> fmt::Debug for ResponseChecksumInterceptor<VE, CM> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseChecksumInterceptor")
            .field("response_algorithms", &self.response_algorithms)
            .finish()
    }
}

impl<VE, CM> ResponseChecksumInterceptor<VE, CM> {
    pub(crate) fn new(
        response_algorithms: &'static [&'static str],
        validation_enabled: VE,
        checksum_mutator: CM,
    ) -> Self {
        Self {
            response_algorithms,
            validation_enabled,
            checksum_mutator,
        }
    }
}

impl<VE, CM> Intercept for ResponseChecksumInterceptor<VE, CM>
where
    VE: Fn(&Input) -> bool + Send + Sync,
    CM: Fn(&mut Input, &ConfigBag) -> Result<(), BoxError> + Send + Sync,
{
    fn name(&self) -> &'static str {
        "ResponseChecksumInterceptor"
    }

    fn modify_before_serialization(
        &self,
        context: &mut BeforeSerializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        (self.checksum_mutator)(context.input_mut(), cfg)?;
        let validation_enabled = (self.validation_enabled)(context.input());

        let mut layer = Layer::new("ResponseChecksumInterceptor");
        layer.store_put(ResponseChecksumInterceptorState { validation_enabled });
        cfg.push_layer(layer);

        let response_checksum_validation = cfg
            .load::<ResponseChecksumValidation>()
            .unwrap_or(&ResponseChecksumValidation::WhenSupported);

        // Set the user-agent feature metric for the response checksum config
        match response_checksum_validation {
            ResponseChecksumValidation::WhenSupported => {
                cfg.interceptor_state()
                    .store_append(SmithySdkFeature::FlexibleChecksumsResWhenSupported);
            }
            ResponseChecksumValidation::WhenRequired => {
                cfg.interceptor_state()
                    .store_append(SmithySdkFeature::FlexibleChecksumsResWhenRequired);
            }
            unsupported => tracing::warn!(
                more_info = "Unsupported value of ResponseChecksumValidation when setting user-agent metrics",
                unsupported = ?unsupported),
        };

        Ok(())
    }

    fn modify_before_deserialization(
        &self,
        context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let state = cfg
            .load::<ResponseChecksumInterceptorState>()
            .expect("set in `read_before_serialization`");

        // This value is set by the user on the SdkConfig to indicate their preference
        // We provide a default here for users that use a client config instead of the SdkConfig
        let response_checksum_validation = cfg
            .load::<ResponseChecksumValidation>()
            .unwrap_or(&ResponseChecksumValidation::WhenSupported);

        // If validation has not been explicitly enabled we check the ResponseChecksumValidation
        // from the SdkConfig. If it is WhenSupported (or unknown) we enable validation and if it
        // is WhenRequired we leave it disabled since there is no way to indicate that a response
        // checksum is required.
        let validation_enabled = if !state.validation_enabled {
            match response_checksum_validation {
                ResponseChecksumValidation::WhenRequired => false,
                ResponseChecksumValidation::WhenSupported => true,
                _ => true,
            }
        } else {
            true
        };

        if validation_enabled {
            let response = context.response_mut();
            let maybe_checksum_headers = check_headers_for_precalculated_checksum(
                response.headers(),
                self.response_algorithms,
            );

            if let Some((checksum_algorithm, precalculated_checksum)) = maybe_checksum_headers {
                let mut body = SdkBody::taken();
                mem::swap(&mut body, response.body_mut());

                let mut body = wrap_body_with_checksum_validator(
                    body,
                    checksum_algorithm,
                    precalculated_checksum,
                );
                mem::swap(&mut body, response.body_mut());
            }
        }

        Ok(())
    }
}

/// Given an `SdkBody`, a `aws_smithy_checksums::ChecksumAlgorithm`, and a pre-calculated checksum,
/// return an `SdkBody` where the body will processed with the checksum algorithm and checked
/// against the pre-calculated checksum.
pub(crate) fn wrap_body_with_checksum_validator(
    body: SdkBody,
    checksum_algorithm: ChecksumAlgorithm,
    precalculated_checksum: bytes::Bytes,
) -> SdkBody {
    use aws_smithy_checksums::body::validate;

    body.map(move |body: SdkBody| {
        SdkBody::from_body_1_x(validate::ChecksumBody::new(
            body,
            checksum_algorithm.into_impl(),
            precalculated_checksum.clone(),
        ))
    })
}

/// Given a `HeaderMap`, extract any checksum included in the headers as `Some(Bytes)`.
/// If no checksum header is set, return `None`. If multiple checksum headers are set, the one that
/// is fastest to compute will be chosen.
pub(crate) fn check_headers_for_precalculated_checksum(
    headers: &Headers,
    response_algorithms: &[&str],
) -> Option<(ChecksumAlgorithm, bytes::Bytes)> {
    let checksum_algorithms_to_check =
        aws_smithy_checksums::http::CHECKSUM_ALGORITHMS_IN_PRIORITY_ORDER
            .into_iter()
            // Process list of algorithms, from fastest to slowest, that may have been used to checksum
            // the response body, ignoring any that aren't marked as supported algorithms by the model.
            .flat_map(|algo| {
                // For loop is necessary b/c the compiler doesn't infer the correct lifetimes for iter().find()
                for res_algo in response_algorithms {
                    if algo.eq_ignore_ascii_case(res_algo) {
                        return Some(algo);
                    }
                }

                None
            });

    for checksum_algorithm in checksum_algorithms_to_check {
        let checksum_algorithm: ChecksumAlgorithm = checksum_algorithm.parse().expect(
            "CHECKSUM_ALGORITHMS_IN_PRIORITY_ORDER only contains valid checksum algorithm names",
        );
        if let Some(base64_encoded_precalculated_checksum) =
            headers.get(checksum_algorithm.into_impl().header_name())
        {
            // S3 needs special handling for checksums of objects uploaded with `MultiPartUpload`.
            if is_part_level_checksum(base64_encoded_precalculated_checksum) {
                tracing::warn!(
                      more_info = "See https://docs.aws.amazon.com/AmazonS3/latest/userguide/checking-object-integrity.html#large-object-checksums for more information.",
                      "This checksum is a part-level checksum which can't be validated by the Rust SDK. Disable checksum validation for this request to fix this warning.",
                  );

                return None;
            }

            let precalculated_checksum = match aws_smithy_types::base64::decode(
                base64_encoded_precalculated_checksum,
            ) {
                Ok(decoded_checksum) => decoded_checksum.into(),
                Err(_) => {
                    tracing::error!("Checksum received from server could not be base64 decoded. No checksum validation will be performed.");
                    return None;
                }
            };

            return Some((checksum_algorithm, precalculated_checksum));
        }
    }

    None
}

fn is_part_level_checksum(checksum: &str) -> bool {
    let mut found_number = false;
    let mut found_dash = false;

    for ch in checksum.chars().rev() {
        // this could be bad
        if ch.is_ascii_digit() {
            found_number = true;
            continue;
        }

        // We saw a number first followed by the dash, yup, it's a part-level checksum
        if found_number && ch == '-' {
            if found_dash {
                // Found a second dash?? This isn't a part-level checksum.
                return false;
            }

            found_dash = true;
            continue;
        }

        break;
    }

    found_number && found_dash
}

#[cfg(test)]
mod tests {
    use super::{is_part_level_checksum, wrap_body_with_checksum_validator};
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::byte_stream::ByteStream;
    use aws_smithy_types::error::display::DisplayErrorContext;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_build_checksum_validated_body_works() {
        let checksum_algorithm = "crc32".parse().unwrap();
        let input_text = "Hello world";
        let precalculated_checksum = Bytes::from_static(&[0x8b, 0xd6, 0x9e, 0x52]);
        let body = ByteStream::new(SdkBody::from(input_text));

        let body = body.map(move |sdk_body| {
            wrap_body_with_checksum_validator(
                sdk_body,
                checksum_algorithm,
                precalculated_checksum.clone(),
            )
        });

        let mut validated_body = Vec::new();
        if let Err(e) = tokio::io::copy(&mut body.into_async_read(), &mut validated_body).await {
            tracing::error!("{}", DisplayErrorContext(&e));
            panic!("checksum validation has failed");
        };
        let body = std::str::from_utf8(&validated_body).unwrap();

        assert_eq!(input_text, body);
    }

    #[test]
    fn test_is_multipart_object_checksum() {
        // These ARE NOT part-level checksums
        assert!(!is_part_level_checksum("abcd"));
        assert!(!is_part_level_checksum("abcd="));
        assert!(!is_part_level_checksum("abcd=="));
        assert!(!is_part_level_checksum("1234"));
        assert!(!is_part_level_checksum("1234="));
        assert!(!is_part_level_checksum("1234=="));
        // These ARE part-level checksums
        assert!(is_part_level_checksum("abcd-1"));
        assert!(is_part_level_checksum("abcd=-12"));
        assert!(is_part_level_checksum("abcd12-134"));
        assert!(is_part_level_checksum("abcd==-10000"));
        // These are gibberish and shouldn't be regarded as a part-level checksum
        assert!(!is_part_level_checksum(""));
        assert!(!is_part_level_checksum("Spaces? In my header values?"));
        assert!(!is_part_level_checksum("abcd==-134!#{!#"));
        assert!(!is_part_level_checksum("abcd==-"));
        assert!(!is_part_level_checksum("abcd==--11"));
        assert!(!is_part_level_checksum("abcd==-AA"));
    }

    #[test]
    fn part_level_checksum_detection_works() {
        let a_real_checksum = is_part_level_checksum("C9A5A6878D97B48CC965C1E41859F034-14");
        assert!(a_real_checksum);
        let close_but_not_quite = is_part_level_checksum("a4-");
        assert!(!close_but_not_quite);
        let backwards = is_part_level_checksum("14-C9A5A6878D97B48CC965C1E41859F034");
        assert!(!backwards);
        let double_dash = is_part_level_checksum("C9A5A6878D97B48CC965C1E41859F03-4-14");
        assert!(!double_dash);
    }
}
