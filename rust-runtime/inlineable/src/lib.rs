/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#[allow(dead_code)]
mod aws_json_errors;
mod blob_serde;
#[allow(dead_code)]
mod doc_json;
#[allow(dead_code)]
mod idempotency_token;
mod instant_epoch;
mod instant_httpdate;
mod instant_iso8601;

// This test is outside of uuid.rs to enable copying the entirety of uuid.rs into the SDK without
// requiring a proptest dependency
#[cfg(test)]
mod test {
    use crate::doc_json::SerDoc;
    use crate::idempotency_token;
    use crate::idempotency_token::uuid_v4;
    use proptest::prelude::*;
    use proptest::std_facade::HashMap;
    use smithy_types::Document;
    use smithy_types::Number;
    use std::sync::Mutex;

    #[test]
    fn nan_floats_serialize_null() {
        let mut map = HashMap::new();
        map.insert("num".to_string(), Document::Number(Number::PosInt(45)));
        map.insert("nan".to_string(), Document::Number(Number::Float(f64::NAN)));
        let doc = Document::Object(map);
        assert_eq!(
            serde_json::to_value(&SerDoc(&doc)).unwrap(),
            serde_json::json!({"num":45,"nan":null})
        );
    }

    #[test]
    fn test_uuid() {
        assert_eq!(uuid_v4(0), "00000000-0000-4000-8000-000000000000");
        assert_eq!(uuid_v4(12341234), "2ff4cb00-0000-4000-8000-000000000000");
        assert_eq!(
            uuid_v4(u128::max_value()),
            "ffffffff-ffff-4fff-ffff-ffffffffffff"
        );
    }

    #[test]
    fn default_token_generator_smoke_test() {
        // smoke test to make sure the default token generator produces a token-like object
        use crate::idempotency_token::MakeIdempotencyToken;
        assert_eq!(idempotency_token::default_provider().make_idempotency_token().len(), 36);
    }

    #[test]
    fn token_generator() {
        let provider = Mutex::new(fastrand::Rng::with_seed(123));
        use crate::idempotency_token::MakeIdempotencyToken;
        assert_eq!(provider.make_idempotency_token(), "b4021a03-ae07-4db5-fc1b-38bf919691f8");
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
