/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::schema::routing::RouterBuildError;
use aws_smithy_http_server::schema::settings::{get, parse_settings_json, FromSetting};
use aws_smithy_types::{Document, Number};

fn section(value: Document) -> Document {
    Document::Object([("value".to_owned(), value)].into())
}

fn assert_invalid<'a, T: FromSetting<'a>>(settings: &'a Document, actual: &Document) {
    let Err(RouterBuildError::Configuration(message)) = get::<T>(Some(settings), "value") else {
        panic!("expected a configuration error");
    };
    assert!(message.contains("`value`"), "{message}");
    assert!(message.contains(T::EXPECTED), "{message}");
    assert!(message.contains(&format!("{actual:?}")), "{message}");
}

#[test]
fn absent_sections_and_keys_have_no_default() {
    let empty = parse_settings_json(b"{}");
    for settings in [None, Some(&empty)] {
        assert_eq!(get::<bool>(settings, "value").unwrap(), None);
        assert_eq!(get::<u64>(settings, "value").unwrap(), None);
        assert_eq!(get::<&str>(settings, "value").unwrap(), None);
        assert_eq!(get::<String>(settings, "value").unwrap(), None);
    }
}

#[test]
fn built_in_types_accept_valid_values() {
    for value in [false, true] {
        assert_eq!(
            get::<bool>(Some(&section(Document::Bool(value))), "value").unwrap(),
            Some(value)
        );
    }
    for value in [0, 42, u64::MAX] {
        assert_eq!(
            get::<u64>(Some(&section(Document::Number(Number::PosInt(value)))), "value").unwrap(),
            Some(value)
        );
    }
    for value in ["", "settings"] {
        let settings = section(Document::String(value.to_owned()));
        assert_eq!(get::<String>(Some(&settings), "value").unwrap().as_deref(), Some(value));
        let borrowed = get::<&str>(Some(&settings), "value").unwrap().unwrap();
        let Document::Object(object) = &settings else {
            unreachable!()
        };
        let stored = object["value"].as_string().unwrap();
        assert_eq!(borrowed, value);
        assert_eq!(borrowed.as_ptr(), stored.as_ptr());
    }
}

#[test]
fn invalid_sections_report_key_expected_type_and_actual_value() {
    for settings in [
        Document::Null,
        Document::Bool(true),
        Document::String("invalid".into()),
        Document::Array(vec![]),
        Document::Number(Number::PosInt(1)),
    ] {
        assert_invalid::<bool>(&settings, &settings);
        assert_invalid::<u64>(&settings, &settings);
        assert_invalid::<&str>(&settings, &settings);
        assert_invalid::<String>(&settings, &settings);
    }
}

#[test]
fn wrong_types_and_explicit_null_are_invalid() {
    let values = [
        Document::Null,
        Document::Bool(true),
        Document::Number(Number::PosInt(1)),
        Document::String("1".into()),
        Document::Array(vec![]),
        Document::Object(Default::default()),
    ];
    for value in values {
        let settings = section(value.clone());
        if !matches!(value, Document::Bool(_)) {
            assert_invalid::<bool>(&settings, &value);
        }
        if !matches!(value, Document::Number(_)) {
            assert_invalid::<u64>(&settings, &value);
        }
        if !matches!(value, Document::String(_)) {
            assert_invalid::<&str>(&settings, &value);
            assert_invalid::<String>(&settings, &value);
        }
    }
}

#[test]
fn unsigned_integers_require_pos_int() {
    for number in [
        Number::NegInt(-1),
        Number::NegInt(0),
        Number::Float(0.0),
        Number::Float(1.0),
        Number::Float(1.5),
    ] {
        let value = Document::Number(number);
        assert_invalid::<u64>(&section(value.clone()), &value);
    }
}

#[derive(Debug, PartialEq)]
struct Mode<'a>(&'a str);

impl<'a> FromSetting<'a> for Mode<'a> {
    const EXPECTED: &'static str = "the string `custom`";

    fn from_setting(value: &'a Document) -> Option<Self> {
        value.as_string().filter(|value| *value == "custom").map(Self)
    }
}

#[test]
fn downstream_types_can_define_borrowing_conversions() {
    let settings = parse_settings_json(br#"{"value":"custom"}"#);
    assert_eq!(get::<Mode<'_>>(Some(&settings), "value").unwrap(), Some(Mode("custom")));
    let value = Document::String("unsupported".into());
    assert_invalid::<Mode<'_>>(&section(value.clone()), &value);
}

#[test]
fn parser_accepts_a_single_document() {
    assert_eq!(parse_settings_json(br#"{"value":true}"#), section(Document::Bool(true)));
}

#[test]
#[should_panic(expected = "codegen emits well-formed settings JSON")]
fn parser_panics_for_malformed_codegen_output() {
    parse_settings_json(b"{");
}

#[test]
#[should_panic(expected = "codegen emits a single settings JSON document")]
fn parser_panics_for_multiple_documents() {
    parse_settings_json(b"{} {}");
}
