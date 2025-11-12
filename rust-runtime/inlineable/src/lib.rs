/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#[allow(dead_code)]
mod aws_query_compatible_errors;
#[allow(unused)]
mod cbor_errors;
#[allow(unused)]
mod client_http_checksum_required;
#[allow(dead_code)]
mod client_idempotency_token;
#[allow(unused)]
mod constrained;
#[allow(dead_code)]
mod ec2_query_errors;
#[allow(unused)]
mod event_receiver;
#[allow(dead_code)]
mod idempotency_token;
#[allow(dead_code)]
mod json_errors;
#[allow(unused)]
mod rest_xml_unwrapped_errors;
#[allow(unused)]
mod rest_xml_wrapped_errors;
#[allow(dead_code)]
mod sdk_feature_tracker;
#[allow(unused)]
mod serialization_settings;

#[allow(unused)]
mod endpoint_lib;

#[allow(unused)]
mod auth_plugin;

#[allow(unused)]
mod client_request_compression;

// This test is outside of uuid.rs to enable copying the entirety of uuid.rs into the SDK without
// requiring a proptest dependency
#[cfg(test)]
mod test {
    use crate::idempotency_token;
    use crate::idempotency_token::{uuid_v4, IdempotencyTokenProvider};
    use proptest::prelude::*;
    use regex_lite::Regex;

    #[test]
    fn test_uuid() {
        assert_eq!(uuid_v4(0), "00000000-0000-4000-8000-000000000000");
        assert_eq!(uuid_v4(12341234), "2ff4cb00-0000-4000-8000-000000000000");
        assert_eq!(uuid_v4(u128::MAX), "ffffffff-ffff-4fff-ffff-ffffffffffff");
    }

    #[test]
    fn default_token_generator_smoke_test() {
        // smoke test to make sure the default token generator produces a token-like object
        assert_eq!(
            idempotency_token::default_provider()
                .make_idempotency_token()
                .len(),
            36
        );
    }

    #[test]
    fn token_generator() {
        let provider = IdempotencyTokenProvider::random();
        let token = provider.make_idempotency_token();
        assert!(
            Regex::new(
                r"[A-Fa-f0-9]{8}-[A-Fa-f0-9]{4}-4[A-Fa-f0-9]{3}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{12}"
            )
            .unwrap()
            .is_match(&token),
            "token {token} wasn't a valid random UUID"
        );
    }

    fn assert_valid(uuid: String) {
        assert_eq!(uuid.len(), 36);
        let bytes = uuid.as_bytes();
        let dashes: Vec<usize> = uuid
            .chars()
            .enumerate()
            .filter_map(|(idx, chr)| if chr == '-' { Some(idx) } else { None })
            .collect();
        assert_eq!(dashes, vec![8, 13, 18, 23]);
        // Check version
        assert_eq!(bytes[14] as char, '4');
        // Check variant
        assert!(bytes[19] as char >= '8');
    }

    proptest! {
        #[test]
        fn doesnt_crash_uuid(v in any::<u128>()) {
            let uuid = uuid_v4(v);
            assert_valid(uuid);
        }
    }
}
