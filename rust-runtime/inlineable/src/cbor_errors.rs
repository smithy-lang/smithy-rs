/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_cbor::decode::DeserializeError;
use aws_smithy_cbor::Decoder;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_types::error::metadata::{Builder as ErrorMetadataBuilder, ErrorMetadata};

fn sanitize_error_code(error_code: &str) -> &str {
    // Trim a trailing URL from the error code, beginning with a `:`
    let error_code = match error_code.find(':') {
        Some(idx) => &error_code[..idx],
        None => error_code,
    };

    // Trim a prefixing namespace from the error code, beginning with a `#`
    match error_code.find('#') {
        Some(idx) => &error_code[idx + 1..],
        None => error_code,
    }
}

pub fn parse_error_metadata(
    _response_status: u16,
    _response_headers: &Headers,
    response_body: &[u8],
) -> Result<ErrorMetadataBuilder, DeserializeError> {
    fn error_code(
        mut builder: ErrorMetadataBuilder,
        decoder: &mut Decoder,
    ) -> Result<ErrorMetadataBuilder, DeserializeError> {
        builder = match decoder.str()?.as_ref() {
            "__type" => {
                let code = decoder.str()?;
                builder.code(sanitize_error_code(&code))
            }
            _ => {
                decoder.skip()?;
                builder
            }
        };
        Ok(builder)
    }

    let decoder = &mut Decoder::new(response_body);
    let mut builder = ErrorMetadata::builder();

    match decoder.map()? {
        None => loop {
            match decoder.datatype()? {
                ::aws_smithy_cbor::data::Type::Break => {
                    decoder.skip()?;
                    break;
                }
                _ => {
                    builder = error_code(builder, decoder)?;
                }
            };
        },
        Some(n) => {
            for _ in 0..n {
                builder = error_code(builder, decoder)?;
            }
        }
    };

    Ok(builder)
}
