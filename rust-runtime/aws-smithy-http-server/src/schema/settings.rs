/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed access to global and protocol settings.

use aws_smithy_types::{Document, Number};

use crate::schema::routing::RouterBuildError;

/// Converts a settings value into a type, optionally borrowing from the document.
///
/// Protocols can implement this trait for their own settings types. Return `None`
/// when the value is invalid; [`get`] reports it as a configuration error.
pub trait FromSetting<'a>: Sized {
    /// The expected type or value, used in configuration error diagnostics.
    const EXPECTED: &'static str;

    /// Converts a value, returning `None` if it does not match this type.
    fn from_setting(value: &'a Document) -> Option<Self>;
}

impl FromSetting<'_> for bool {
    const EXPECTED: &'static str = "a boolean";

    fn from_setting(value: &Document) -> Option<Self> {
        value.as_bool()
    }
}

impl FromSetting<'_> for u64 {
    const EXPECTED: &'static str = "a non-negative integer";

    fn from_setting(value: &Document) -> Option<Self> {
        match value {
            Document::Number(Number::PosInt(value)) => Some(*value),
            _ => None,
        }
    }
}

impl<'a> FromSetting<'a> for &'a str {
    const EXPECTED: &'static str = "a string";

    fn from_setting(value: &'a Document) -> Option<Self> {
        value.as_string()
    }
}

impl FromSetting<'_> for String {
    const EXPECTED: &'static str = "a string";

    fn from_setting(value: &Document) -> Option<Self> {
        value.as_string().map(str::to_owned)
    }
}

/// Reads a typed value from a settings object.
///
/// An absent section or key returns `Ok(None)`, leaving defaults to callers.
/// A non-object section or an invalid value (including explicit `null`) returns
/// [`RouterBuildError::Configuration`].
pub fn get<'a, T: FromSetting<'a>>(settings: Option<&'a Document>, key: &str) -> Result<Option<T>, RouterBuildError> {
    let Some(settings) = settings else {
        return Ok(None);
    };
    let Document::Object(object) = settings else {
        return Err(RouterBuildError::Configuration(format!(
            "settings section for `{key}` (expected {}) must be a JSON object, got {settings:?}",
            T::EXPECTED,
        )));
    };
    object
        .get(key)
        .map(|value| {
            T::from_setting(value).ok_or_else(|| {
                RouterBuildError::Configuration(format!("setting `{key}` must be {}, got {value:?}", T::EXPECTED,))
            })
        })
        .transpose()
}

/// Parses a JSON object emitted by codegen into a settings [`Document`].
///
/// Generated `routing_options()` functions embed each protocol's section of
/// `customizationConfig.protocols` as a JSON byte-string and call this once at
/// service build time. The input is printed by codegen from a validated node,
/// so malformed JSON is a codegen bug: this panics rather than returning an
/// error.
///
/// [`Document`]: aws_smithy_types::Document
pub fn parse_settings_json(json: &[u8]) -> aws_smithy_types::Document {
    let mut tokens = aws_smithy_json::deserialize::json_token_iter(json).peekable();
    let document = aws_smithy_json::deserialize::token::expect_document(&mut tokens)
        .expect("codegen emits well-formed settings JSON");
    assert!(tokens.next().is_none(), "codegen emits a single settings JSON document");
    document
}
