/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Every error a caller can see names the line and column it came from.

use aws_smithy_lang::shape::{Shape, StructureShape};
use aws_smithy_lang::{Model, SourceLocation};
use std::path::Path;

const MODEL: &str = r#"{
    "smithy": "2.0",
    "shapes": {
        "ex#Op": {
            "type": "operation",
            "input": { "target": "ex#Input" },
            "errors": [
                { "target": "ex#Missing" }
            ]
        },
        "ex#Input": {
            "type": "structure",
            "members": {
                "name": {
                    "target": "smithy.api#String"
                }
            }
        }
    }
}"#;

/// [`MODEL`] without its unresolved error reference.
fn valid() -> String {
    MODEL.replace(r#"{ "target": "ex#Missing" }"#, "")
}

/// The text at a location's line and column.
fn text_at<'a>(input: &'a str, location: &SourceLocation) -> &'a str {
    let line = input
        .lines()
        .nth(location.line().expect("a line") as usize - 1)
        .expect("the line exists");
    let column = location.column().expect("a column") as usize - 1;
    let start = line
        .char_indices()
        .nth(column)
        .map_or(line.len(), |(i, _)| i);
    &line[start..]
}

#[test]
fn reference_errors_point_at_the_reference() {
    let err = Model::from_json_str("m.json", MODEL).unwrap_err();
    let diagnostic = err.diagnostics().iter().next().unwrap();
    let location = diagnostic.source_location().unwrap();
    assert_eq!((location.line(), location.column()), (Some(8), Some(19)));
    assert_eq!(text_at(MODEL, location), r#""target": "ex#Missing" }"#);
    assert_eq!(
        diagnostic.to_string(),
        "m.json:8:19: error[UnresolvedReference]: `ex#Missing` does not resolve to a shape \
         (at /shapes/ex#Op/errors/0/target)"
    );
    assert_eq!(
        err.to_string(),
        "invalid Smithy model at m.json:8:19: `ex#Missing` does not resolve to a shape"
    );
}

#[test]
fn every_java_fixture_error_points_at_its_value() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java/invalid");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let input = std::fs::read_to_string(&path).unwrap();
        let err = Model::from_json_file(&path).unwrap_err();
        for diagnostic in err.diagnostics().iter() {
            let location = diagnostic.source_location().unwrap();
            assert_eq!(location.source(), path.display().to_string());
            // The position is the key that introduces the value the pointer names.
            let pointer = location.pointer().unwrap();
            let key = pointer
                .rsplit('/')
                .next()
                .unwrap()
                .replace("~1", "/")
                .replace("~0", "~");
            let text = text_at(&input, location);
            if key.parse::<usize>().is_err() {
                assert!(
                    text.starts_with(&format!("\"{key}\"")),
                    "{diagnostic}: {text}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 18, "checked {checked} diagnostics");
}

#[test]
fn escaped_keys_use_the_first_nested_key() {
    let json = "{\n  \"smithy\": \"2.0\",\n  \"shapes\": {\n    \"ex#\\u0041\": { \"type\": \"string\" }\n  }\n}";
    let model = Model::from_json_str("m.json", json).unwrap();
    let location = model
        .expect_shape("ex#A")
        .unwrap()
        .source_location()
        .clone();
    // serde_json unescapes the key into a buffer, so its position is unknown; the shape's
    // object is positioned at its first key instead.
    assert_eq!(location.to_string(), "m.json:4:20 (/shapes/ex#A)");
}

#[test]
fn columns_count_characters() {
    let json = r#"{"smithy": "2.0", "metadata": {"é": "ü", "k": 1}}"#;
    let model = Model::from_json_str("m.json", json).unwrap();
    let k = model.metadata_value("k").unwrap();
    assert_eq!(k.source_location().column(), Some(42));
    assert_eq!(text_at(json, k.source_location()), r#""k": 1}}"#);
}

#[test]
fn readers_and_multiple_sources_have_positions() {
    let other = "{\n  \"smithy\": \"2.0\",\n  \"shapes\": {\n    \"ex#Op\": { \"type\": \"string\" }\n  }\n}";
    let err = Model::assembler()
        .add_json_reader("a.json", valid().as_bytes())
        .add_json_str("b.json", other)
        .assemble()
        .into_result()
        .unwrap_err();
    assert_eq!(
        err.diagnostics().iter().next().unwrap().to_string(),
        "b.json:4:5: error[ShapeConflict]: conflicting definitions of `ex#Op` \
         (at /shapes/ex#Op; see also a.json:4:9)"
    );
}

#[test]
fn syntax_errors_report_the_position_once() {
    let err = Model::from_json_str("m.json", "{\n  \"smithy\": \"2.0\",\n  oops\n}").unwrap_err();
    assert_eq!(
        err.to_string(),
        "failed to parse Smithy JSON AST at m.json:3:3"
    );
    let diagnostic = err.diagnostics().iter().next().unwrap();
    assert_eq!(diagnostic.message(), "key must be a string");
    assert_eq!(
        diagnostic.to_string(),
        "m.json:3:3: error[JsonSyntax]: key must be a string"
    );
}

#[test]
fn multiple_errors_name_the_first() {
    let json = MODEL.replace("ex#Input\" }", "ex#Gone\" }");
    let err = Model::from_json_str("m.json", &json).unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid Smithy model (2 errors); first at m.json:6:24: \
         `ex#Gone` does not resolve to a shape"
    );
}

#[test]
fn view_and_node_errors_show_locations() {
    let model = Model::from_json_str("m.json", &valid()).unwrap();
    let err = model
        .expect_shape("ex#Input")
        .unwrap()
        .expect_service()
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "m.json:11:9: expected `ex#Input` to be a service, but it is a structure \
         (at /shapes/ex#Input)"
    );
    assert_eq!(err.source_location().line(), Some(11));

    let member = model.expect_shape("ex#Input$name").unwrap();
    let err = member.expect_shape().unwrap_err();
    assert!(err.to_string().starts_with("m.json:14:17: "), "{err}");

    // A builder made from a loaded shape reports the shape's location.
    let Shape::Structure(input) = model
        .expect_shape("ex#Input")
        .unwrap()
        .expect_shape()
        .unwrap()
        .shape()
        .clone()
    else {
        panic!("a structure");
    };
    let err = input
        .to_builder()
        .member("bad name", "smithy.api#String")
        .build()
        .unwrap_err();
    assert!(
        err.to_string().starts_with("m.json:11:9: invalid shape: "),
        "{err}"
    );
    assert!(StructureShape::builder("ex#A")
        .member("bad name", "smithy.api#String")
        .build()
        .unwrap_err()
        .to_string()
        .starts_with("invalid shape: "));
}

#[test]
fn node_type_errors_show_locations() {
    let json = "{\n  \"smithy\": \"2.0\",\n  \"metadata\": {\n    \"k\": 1\n  }\n}";
    let model = Model::from_json_str("m.json", json).unwrap();
    let err = model.metadata_value("k").unwrap().expect_str().unwrap_err();
    assert_eq!(
        err.to_string(),
        "m.json:4:5: expected string node but found number (at /metadata/k)"
    );
}
