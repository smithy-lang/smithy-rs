/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Malformed input never panics and never produces a partial model.

use aws_smithy_lang::diagnostic::{DiagnosticCode, Severity};
use aws_smithy_lang::{LoadError, Model, ModelLoader, ModelWriter};
use proptest::prelude::*;

const VALID: &str = r#"{"smithy":"2.0","metadata":{"m":[1,{"x":null}]},"shapes":{
    "a#Svc":{"type":"service","operations":[{"target":"a#Op"}]},
    "a#Op":{"type":"operation","input":{"target":"a#In"},"errors":[{"target":"a#Err"}]},
    "a#In":{"type":"structure","members":{"s":{"target":"smithy.api#String","traits":{"smithy.api#required":{}}},
            "l":{"target":"a#L"}}},
    "a#L":{"type":"list","member":{"target":"a#E"}},
    "a#E":{"type":"enum","members":{"A":{"target":"smithy.api#Unit"}}},
    "a#Err":{"type":"structure","traits":{"smithy.api#error":"client"}}
}}"#;

/// Loads `input` and checks the result is self-consistent: an error always carries at least
/// one error diagnostic, and a model always serializes and reloads to an equivalent model.
fn check(input: &[u8]) -> Result<Model, LoadError> {
    let result = Model::from_json_slice("fuzz", input);
    match &result {
        Ok(model) => {
            let out = ModelWriter::new().to_string(model).unwrap();
            let reloaded = Model::from_json_str("reload", &out).expect("written output reloads");
            assert!(model.equivalent(&reloaded));
        }
        Err(err) => {
            assert!(err.diagnostics().has_errors(), "{err}");
            assert!(err
                .diagnostics()
                .iter()
                .all(|d| d.severity() == Severity::Error));
            let _ = err.to_string();
        }
    }
    result
}

fn codes(err: &LoadError) -> Vec<DiagnosticCode> {
    err.diagnostics().iter().map(|d| d.code()).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes(input in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = check(&input);
    }

    #[test]
    fn truncated_documents(len in 0..VALID.len()) {
        prop_assert!(check(&VALID.as_bytes()[..len]).is_err());
    }

    #[test]
    fn mutated_documents(
        edits in prop::collection::vec((0..VALID.len(), any::<u8>()), 1..4),
    ) {
        let mut input = VALID.as_bytes().to_vec();
        for (index, byte) in edits {
            input[index] = byte;
        }
        let _ = check(&input);
    }

    #[test]
    fn structural_mutations(
        index in 0..VALID.len(),
        insert in prop::sample::select(vec![
            "{", "}", "[", "]", ",", ":", "\"", "null", "\"type\":\"apply\",", "1e999",
            "\"a#X\":{\"type\":\"string\"},", "{\"target\":\"a#Missing\"}",
        ]),
    ) {
        let mut input = VALID.to_owned();
        if input.is_char_boundary(index) {
            input.insert_str(index, insert);
            let _ = check(input.as_bytes());
        }
    }
}

#[test]
fn valid_baseline() {
    assert!(check(VALID.as_bytes()).is_ok());
}

#[test]
fn any_error_means_no_model() {
    // One unresolved reference among otherwise valid shapes fails the whole load.
    let input = VALID.replace(r#""target":"a#E""#, r#""target":"a#Nope""#);
    let err = check(input.as_bytes()).unwrap_err();
    assert!(err.is_invalid_model());
    assert_eq!(codes(&err), [DiagnosticCode::UnresolvedReference]);
}

#[test]
fn every_mixin_and_apply_form_is_unsupported() {
    let cases = [
        r#""a#X":{"type":"apply","traits":{"smithy.api#documentation":"x"}}"#,
        r#""a#X":{"type":"structure","mixins":[{"target":"a#M"}]},"a#M":{"type":"structure"}"#,
        r#""a#M":{"type":"structure","traits":{"smithy.api#mixin":{}}}"#,
        r#""a#M":{"type":"string","traits":{"smithy.api#mixin":{"localTraits":[]}}}"#,
        r#""a#M":{"type":"operation","mixins":[{"target":"a#O"}]},"a#O":{"type":"operation"}"#,
    ];
    for shapes in cases {
        let input = format!(r#"{{"smithy":"2.0","shapes":{{{shapes}}}}}"#);
        let err = check(input.as_bytes()).unwrap_err();
        assert!(
            codes(&err).contains(&DiagnosticCode::UnsupportedFeature),
            "{shapes}"
        );
    }
}

#[test]
fn nested_duplicate_keys() {
    for (from, to) in [
        (r#""m":[1,{"x":null}]"#, r#""m":[1,{"x":null,"x":1}]"#),
        (r#""smithy":"2.0""#, r#""smithy":"2.0","smithy":"2.0""#),
        (r#""type":"list""#, r#""type":"list","type":"list""#),
        (
            r#""smithy.api#required":{}"#,
            r#""smithy.api#required":{},"smithy.api#required":{}"#,
        ),
    ] {
        let input = VALID.replace(from, to);
        assert_ne!(input, VALID);
        let err = check(input.as_bytes()).unwrap_err();
        assert!(err.is_json_syntax(), "{to}");
        assert_eq!(codes(&err), [DiagnosticCode::DuplicateKey], "{to}");
    }
}

#[test]
fn configured_limits() {
    let deep = format!(
        r#"{{"smithy":"2.0","metadata":{{"x":{}0{}}}}}"#,
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    let err = Model::from_json_str("deep", &deep).unwrap_err();
    assert!(err.is_resource_limit());
    let err = Model::from_json_reader("deep", deep.as_bytes()).unwrap_err();
    assert!(err.is_resource_limit());
    assert!(ModelLoader::new()
        .max_depth(3)
        .load_str("v", VALID)
        .unwrap_err()
        .is_resource_limit());
    assert!(ModelLoader::new()
        .max_shapes(3)
        .load_str("v", VALID)
        .unwrap_err()
        .is_resource_limit());
    assert!(ModelLoader::new()
        .max_input_bytes(64)
        .load_reader("v", VALID.as_bytes())
        .unwrap_err()
        .is_resource_limit());

    // An endless reader is bounded by the byte limit rather than read forever.
    let endless = std::io::repeat(b' ');
    let err = ModelLoader::new()
        .max_input_bytes(1 << 16)
        .load_reader("endless", endless)
        .unwrap_err();
    assert!(err.is_resource_limit());
}

#[test]
fn non_object_documents() {
    for input in ["", "null", "[]", "\"x\"", "1", "{", "{}"] {
        let err = check(input.as_bytes()).unwrap_err();
        assert!(!err.diagnostics().is_empty(), "{input:?}");
    }
    assert!(check(br#"{"smithy":"2.0","shapes":[]}"#)
        .unwrap_err()
        .is_json_syntax());
    assert!(check(br#"{"smithy":"2.0","metadata":[]}"#)
        .unwrap_err()
        .is_invalid_model());
    assert!(check(b"\xEF\xBB\xBF{\"smithy\":\"2.0\"}").is_err());
}
