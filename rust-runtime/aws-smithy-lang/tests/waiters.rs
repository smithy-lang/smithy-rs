/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Decodes, inspects, and round-trips `smithy.waiters#waitable` traits.

use aws_smithy_lang::traits::{TraitCodec, TraitDecodeError};
use aws_smithy_lang::waiters::{
    Acceptor, AcceptorState, Matcher, MatcherKind, PathComparator, PathMatcher, Waitable, Waiter,
    WAITABLE,
};
use aws_smithy_lang::{Model, Node, ShapeId};

const OPERATION: ShapeId = ShapeId::from_static("ex#Op");
/// The JSON Pointer of the trait value of `ex#Op`.
const TRAIT: &str = "/shapes/ex#Op/traits/smithy.waiters#waitable";

/// Loads a model in which `ex#Op` has `value` as its `@waitable` trait.
fn load(value: &str) -> Model {
    let json = format!(
        r#"{{"smithy": "2.0", "shapes": {{"ex#Op": {{"type": "operation",
            "traits": {{"smithy.waiters#waitable": {value}}}}}}}}}"#
    );
    Model::from_json_str("m.json", &json).unwrap_or_else(|e| panic!("{e}"))
}

fn trait_value(model: &Model) -> &Node {
    let operation = model.expect_shape(&OPERATION).unwrap();
    operation
        .get_trait(&WAITABLE)
        .expect("the trait is applied")
}

fn decode_model(model: &Model) -> Result<Waitable, TraitDecodeError> {
    let operation = model.expect_shape(&OPERATION).unwrap();
    let waitable = operation.get_trait_as::<Waitable>()?;
    Ok(waitable.expect("the trait is applied"))
}

fn decode(value: &str) -> Result<Waitable, TraitDecodeError> {
    decode_model(&load(value))
}

/// Every property and matcher variant, with properties in the reference implementation's
/// encoding order.
const FULL: &str = r#"{
    "ThingExists": {
        "documentation": "Waits until the thing exists.",
        "acceptors": [
            {
                "state": "success",
                "matcher": {
                    "output": {"path": "thing.status", "expected": "ready", "comparator": "stringEquals"}
                }
            },
            {
                "state": "failure",
                "matcher": {
                    "output": {"path": "thing.status", "expected": "failed", "comparator": "stringEquals"}
                },
                "message": "thing.reason"
            },
            {
                "state": "success",
                "matcher": {
                    "inputOutput": {
                        "path": "input.name == output.thing.name",
                        "expected": "true",
                        "comparator": "booleanEquals"
                    }
                }
            },
            {
                "state": "success",
                "matcher": {
                    "output": {"path": "things[].status", "expected": "ready", "comparator": "allStringEquals"}
                }
            },
            {
                "state": "retry",
                "matcher": {
                    "output": {"path": "things[].status", "expected": "pending", "comparator": "anyStringEquals"}
                }
            },
            {"state": "retry", "matcher": {"errorType": "ThingNotFound"}},
            {"state": "failure", "matcher": {"success": false}}
        ],
        "minDelay": 5,
        "maxDelay": 300,
        "deprecated": true,
        "tags": ["slow", "legacy"]
    },
    "ThingDeleted": {
        "acceptors": [{"state": "success", "matcher": {"errorType": "ex#ThingNotFound"}}]
    }
}"#;

/// A compact description of an acceptor, for comparing whole acceptor lists.
fn describe(acceptor: &Acceptor) -> String {
    let state = match acceptor.state() {
        AcceptorState::Success => "success",
        AcceptorState::Failure => "failure",
        AcceptorState::Retry => "retry",
        other => panic!("unexpected state {other:?}"),
    };
    let path = |matcher: &PathMatcher| {
        format!(
            "{} {:?} {}",
            matcher.path(),
            matcher.comparator(),
            matcher.expected()
        )
    };
    let matcher = match acceptor.matcher().kind() {
        MatcherKind::Output(matcher) => format!("output({})", path(matcher)),
        MatcherKind::InputOutput(matcher) => format!("inputOutput({})", path(matcher)),
        MatcherKind::Success(success) => format!("success({success})"),
        MatcherKind::ErrorType(error) => format!("errorType({error})"),
        other => panic!("unexpected matcher {other:?}"),
    };
    match acceptor.message() {
        Some(message) => format!("{state} {matcher} message({message})"),
        None => format!("{state} {matcher}"),
    }
}

#[test]
fn decodes_every_property_and_matcher_variant() {
    let model = load(FULL);
    let waitable = decode_model(&model).unwrap();
    assert_eq!(waitable.len(), 2);
    assert!(!waitable.is_empty());

    let waiter = waitable.get("ThingExists").unwrap();
    assert_eq!(
        waiter.documentation(),
        Some("Waits until the thing exists.")
    );
    assert_eq!(waiter.min_delay(), 5);
    assert_eq!(waiter.max_delay(), 300);
    assert!(waiter.is_deprecated());
    assert_eq!(waiter.tags(), ["slow", "legacy"]);
    let acceptors: Vec<_> = waiter.acceptors().iter().map(describe).collect();
    assert_eq!(
        acceptors,
        [
            "success output(thing.status StringEquals ready)",
            "failure output(thing.status StringEquals failed) message(thing.reason)",
            "success inputOutput(input.name == output.thing.name BooleanEquals true)",
            "success output(things[].status AllStringEquals ready)",
            "retry output(things[].status AnyStringEquals pending)",
            "retry errorType(ThingNotFound)",
            "failure success(false)",
        ]
    );

    // `errorType` keeps an absolute shape ID exactly as written.
    let deleted = waitable.get("ThingDeleted").unwrap();
    assert_eq!(
        deleted.acceptors()[0].matcher().kind(),
        &MatcherKind::ErrorType("ex#ThingNotFound".into())
    );

    // Encoding reproduces the trait value, including the order of every object's keys.
    let encoded = waitable.to_node();
    assert_eq!(&encoded, trait_value(&model));
    assert_eq!(
        encoded.to_json_string(),
        trait_value(&model).to_json_string()
    );
    assert_eq!(Waitable::from_node(&encoded).unwrap(), waitable);
}

const SUCCESS_ACCEPTOR: &str = r#"{"state": "success", "matcher": {"success": true}}"#;

#[test]
fn applies_defaults_for_omitted_properties() {
    let waitable = decode(&format!(
        r#"{{"W": {{"acceptors": [{SUCCESS_ACCEPTOR}]}}}}"#
    ))
    .unwrap();
    let waiter = waitable.get("W").unwrap();
    assert_eq!(waiter.documentation(), None);
    assert_eq!(waiter.min_delay(), 2);
    assert_eq!(waiter.max_delay(), 120);
    assert!(!waiter.is_deprecated());
    assert!(waiter.tags().is_empty());
    let acceptor = &waiter.acceptors()[0];
    assert_eq!(acceptor.message(), None);
    assert!(acceptor.message_source_location().is_none());

    // Explicit default values decode to the same waiter and are not re-encoded.
    let explicit = decode(&format!(
        r#"{{"W": {{"acceptors": [{SUCCESS_ACCEPTOR}],
            "minDelay": 2, "maxDelay": 120, "deprecated": false, "tags": []}}}}"#
    ))
    .unwrap();
    assert_eq!(explicit, waitable);
    assert_eq!(
        explicit.to_node().to_json_string(),
        r#"{"W":{"acceptors":[{"state":"success","matcher":{"success":true}}]}}"#
    );
}

#[test]
fn accepts_the_delay_bounds() {
    let waitable = decode(&format!(
        r#"{{"W": {{"acceptors": [{SUCCESS_ACCEPTOR}], "minDelay": 1, "maxDelay": 2147483647}}}}"#
    ))
    .unwrap();
    let waiter = waitable.get("W").unwrap();
    assert_eq!(
        (waiter.min_delay(), waiter.max_delay()),
        (1, i32::MAX as u32)
    );
    // Decoding does not compare the delays with each other.
    let inverted = decode(&format!(
        r#"{{"W": {{"acceptors": [{SUCCESS_ACCEPTOR}], "minDelay": 10, "maxDelay": 5}}}}"#
    ))
    .unwrap();
    let waiter = inverted.get("W").unwrap();
    assert_eq!((waiter.min_delay(), waiter.max_delay()), (10, 5));
}

#[test]
fn preserves_waiter_and_acceptor_order() {
    let waiter = r#"{"acceptors": [
        {"state": "retry", "matcher": {"errorType": "Throttled"}},
        {"state": "failure", "matcher": {"success": false}},
        {"state": "success", "matcher": {"success": true}}
    ]}"#;
    let waitable = decode(&format!(
        r#"{{"Zeta": {waiter}, "Alpha": {waiter}, "Mid": {waiter}}}"#
    ))
    .unwrap();
    let names: Vec<_> = waitable.waiters().map(|(name, _)| name).collect();
    assert_eq!(names, ["Zeta", "Alpha", "Mid"]);
    assert_eq!(waitable.waiters().len(), 3);
    let encoded = waitable.to_node();
    let keys: Vec<_> = encoded.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["Zeta", "Alpha", "Mid"]);

    let states: Vec<_> = waitable
        .get("Alpha")
        .unwrap()
        .acceptors()
        .iter()
        .map(Acceptor::state)
        .collect();
    assert_eq!(
        states,
        [
            AcceptorState::Retry,
            AcceptorState::Failure,
            AcceptorState::Success
        ]
    );
    // Lookups are case-sensitive; service-wide case-insensitive uniqueness is a validation rule.
    assert!(waitable.get("alpha").is_none());
}

#[test]
fn equality_ignores_source_locations_and_waiter_order() {
    let located = decode(FULL).unwrap();
    let unlocated = Waitable::from_node(&located.to_node()).unwrap();
    assert!(unlocated.source_location().is_none());
    assert!(!located.source_location().is_none());
    assert_eq!(unlocated, located);

    // Reordering waiters does not change the value, like reordering a node object's keys.
    let a = format!(r#"{{"acceptors": [{SUCCESS_ACCEPTOR}]}}"#);
    let b = r#"{"acceptors": [{"state": "success", "matcher": {"errorType": "E"}}]}"#;
    assert_eq!(
        decode(&format!(r#"{{"A": {a}, "B": {b}}}"#)).unwrap(),
        decode(&format!(r#"{{"B": {b}, "A": {a}}}"#)).unwrap()
    );

    // Reordering acceptors does, because the first matching acceptor wins.
    let retry = r#"{"state": "retry", "matcher": {"errorType": "E"}}"#;
    assert_ne!(
        decode(&format!(
            r#"{{"W": {{"acceptors": [{SUCCESS_ACCEPTOR}, {retry}]}}}}"#
        ))
        .unwrap(),
        decode(&format!(
            r#"{{"W": {{"acceptors": [{retry}, {SUCCESS_ACCEPTOR}]}}}}"#
        ))
        .unwrap()
    );
}

#[test]
fn records_the_location_of_each_value() {
    let waitable = decode(FULL).unwrap();
    let pointer = |location: &aws_smithy_lang::SourceLocation| {
        assert_eq!(location.source(), "m.json");
        assert!(location.line().is_some());
        location
            .pointer()
            .unwrap()
            .strip_prefix(TRAIT)
            .unwrap()
            .to_owned()
    };
    assert_eq!(pointer(waitable.source_location()), "");

    let waiter = waitable.get("ThingExists").unwrap();
    assert_eq!(pointer(waiter.source_location()), "/ThingExists");
    let acceptors = waiter.acceptors();
    assert_eq!(
        pointer(acceptors[1].source_location()),
        "/ThingExists/acceptors/1"
    );
    assert_eq!(
        pointer(acceptors[1].message_source_location().unwrap()),
        "/ThingExists/acceptors/1/message"
    );

    // A matcher is located at its member value; a path matcher also locates its path.
    let matcher: &Matcher = acceptors[1].matcher();
    assert_eq!(
        pointer(matcher.source_location()),
        "/ThingExists/acceptors/1/matcher/output"
    );
    let MatcherKind::Output(output) = matcher.kind() else {
        panic!("expected an output matcher")
    };
    assert_eq!(
        pointer(output.source_location()),
        "/ThingExists/acceptors/1/matcher/output"
    );
    assert_eq!(
        pointer(output.path_source_location()),
        "/ThingExists/acceptors/1/matcher/output/path"
    );
    assert_eq!(
        pointer(acceptors[5].matcher().source_location()),
        "/ThingExists/acceptors/5/matcher/errorType"
    );
    assert_eq!(
        pointer(acceptors[6].matcher().source_location()),
        "/ThingExists/acceptors/6/matcher/success"
    );
}

#[test]
fn ignores_unknown_structure_properties() {
    let with_unknown = decode(
        r#"{"W": {
            "acceptors": [{
                "state": "success",
                "matcher": {"output": {"path": "a", "expected": "b", "comparator": "stringEquals", "extra": 1}},
                "note": "ignored"
            }],
            "future": {}
        }}"#,
    )
    .unwrap();
    let without = decode(
        r#"{"W": {"acceptors": [{
            "state": "success",
            "matcher": {"output": {"path": "a", "expected": "b", "comparator": "stringEquals"}}
        }]}}"#,
    )
    .unwrap();
    assert_eq!(with_unknown, without);
    assert_eq!(
        with_unknown.to_node().to_json_string(),
        without.to_node().to_json_string()
    );
}

#[test]
fn keeps_expressions_exactly_as_written() {
    // Decoding does not parse JMESPath, so syntax errors are left for validation.
    for (path, message) in [
        ("length(items[?state == 'ok']) > `0`", "items[0].reason"),
        ("items[", "join(', ', "),
        ("", ""),
    ] {
        let value = format!(
            r#"{{"W": {{"acceptors": [{{
                "state": "failure",
                "matcher": {{"output": {{"path": {path:?}, "expected": "x", "comparator": "stringEquals"}}}},
                "message": {message:?}
            }}]}}}}"#
        );
        let model = load(&value);
        let waitable = decode_model(&model).unwrap();
        let acceptor = &waitable.get("W").unwrap().acceptors()[0];
        let MatcherKind::Output(output) = acceptor.matcher().kind() else {
            panic!("expected an output matcher")
        };
        assert_eq!(output.path(), path);
        assert_eq!(acceptor.message(), Some(message));
        assert_eq!(&waitable.to_node(), trait_value(&model));
    }
}

fn waiter_with(properties: &str) -> String {
    format!(r#"{{"W": {{"acceptors": [{SUCCESS_ACCEPTOR}], {properties}}}}}"#)
}

fn acceptor(acceptor: &str) -> String {
    format!(r#"{{"W": {{"acceptors": [{acceptor}]}}}}"#)
}

fn matcher(matcher: &str) -> String {
    acceptor(&format!(r#"{{"state": "success", "matcher": {matcher}}}"#))
}

fn output(path_matcher: &str) -> String {
    matcher(&format!(r#"{{"output": {path_matcher}}}"#))
}

#[test]
fn rejects_malformed_values_at_the_offending_value() {
    let cases: Vec<(String, &str, &str)> = vec![
        // The trait value.
        (
            "[]".into(),
            "",
            "expected an object of named waiters, found array",
        ),
        ("{}".into(), "", "expected at least one waiter"),
        (
            format!(r#"{{"thingExists": {{"acceptors": [{SUCCESS_ACCEPTOR}]}}}}"#),
            "/thingExists",
            "waiter name `thingExists` does not match `^[A-Z]+[A-Za-z0-9]*$`",
        ),
        (
            format!(r#"{{"Thing_Exists": {{"acceptors": [{SUCCESS_ACCEPTOR}]}}}}"#),
            "/Thing_Exists",
            "waiter name `Thing_Exists` does not match `^[A-Z]+[A-Za-z0-9]*$`",
        ),
        // Waiters.
        (
            r#"{"W": "waiter"}"#.into(),
            "/W",
            "waiter `W`: expected an object, found string",
        ),
        (
            r#"{"W": {"minDelay": 1}}"#.into(),
            "/W",
            "waiter `W`: missing required member `acceptors`",
        ),
        (
            r#"{"W": {"acceptors": {}}}"#.into(),
            "/W/acceptors",
            "waiter `W`: expected `acceptors` to be an array, found object",
        ),
        (
            r#"{"W": {"acceptors": []}}"#.into(),
            "/W/acceptors",
            "waiter `W`: expected at least one acceptor",
        ),
        (
            waiter_with(r#""documentation": 1"#),
            "/W/documentation",
            "waiter `W`: expected `documentation` to be a string, found number",
        ),
        (
            waiter_with(r#""minDelay": 0"#),
            "/W/minDelay",
            "waiter `W`: expected `minDelay` to be from 1 to 2147483647, found 0",
        ),
        (
            waiter_with(r#""maxDelay": -1"#),
            "/W/maxDelay",
            "waiter `W`: expected `maxDelay` to be from 1 to 2147483647, found -1",
        ),
        (
            waiter_with(r#""maxDelay": 2147483648"#),
            "/W/maxDelay",
            "waiter `W`: expected `maxDelay` to be from 1 to 2147483647, found 2147483648",
        ),
        (
            waiter_with(r#""minDelay": 1.5"#),
            "/W/minDelay",
            "waiter `W`: expected `minDelay` to be an integer, found 1.5",
        ),
        (
            waiter_with(r#""maxDelay": 2.0"#),
            "/W/maxDelay",
            "waiter `W`: expected `maxDelay` to be an integer, found 2.0",
        ),
        (
            waiter_with(r#""minDelay": "2""#),
            "/W/minDelay",
            "waiter `W`: expected `minDelay` to be an integer, found string",
        ),
        (
            waiter_with(r#""deprecated": "yes""#),
            "/W/deprecated",
            "waiter `W`: expected `deprecated` to be a boolean, found string",
        ),
        (
            waiter_with(r#""tags": "slow""#),
            "/W/tags",
            "waiter `W`: expected `tags` to be an array, found string",
        ),
        (
            waiter_with(r#""tags": ["slow", 1]"#),
            "/W/tags/1",
            "waiter `W`: expected a tag string, found number",
        ),
        (
            waiter_with(r#""tags": ["slow", ""]"#),
            "/W/tags/1",
            "waiter `W`: expected a non-empty tag",
        ),
        // Acceptors.
        (
            acceptor(r#""success""#),
            "/W/acceptors/0",
            "waiter `W` acceptor 0: expected an object, found string",
        ),
        (
            acceptor(&format!(
                r#"{SUCCESS_ACCEPTOR}, {{"matcher": {{"success": true}}}}"#
            )),
            "/W/acceptors/1",
            "waiter `W` acceptor 1: missing required member `state`",
        ),
        (
            acceptor(r#"{"state": "done", "matcher": {"success": true}}"#),
            "/W/acceptors/0/state",
            "waiter `W` acceptor 0: unknown acceptor state `done`; \
             expected `success`, `failure`, or `retry`",
        ),
        (
            acceptor(r#"{"state": true, "matcher": {"success": true}}"#),
            "/W/acceptors/0/state",
            "waiter `W` acceptor 0: expected `state` to be a string, found boolean",
        ),
        (
            acceptor(r#"{"state": "success"}"#),
            "/W/acceptors/0",
            "waiter `W` acceptor 0: missing required member `matcher`",
        ),
        (
            acceptor(r#"{"state": "failure", "matcher": {"success": false}, "message": 1}"#),
            "/W/acceptors/0/message",
            "waiter `W` acceptor 0: expected `message` to be a string, found number",
        ),
        // Matchers.
        (
            matcher("[]"),
            "/W/acceptors/0/matcher",
            "waiter `W` acceptor 0: expected `matcher` to be an object, found array",
        ),
        (
            matcher("{}"),
            "/W/acceptors/0/matcher",
            "waiter `W` acceptor 0: expected `matcher` to set exactly one member, found 0",
        ),
        (
            matcher(r#"{"success": true, "errorType": "E"}"#),
            "/W/acceptors/0/matcher",
            "waiter `W` acceptor 0: expected `matcher` to set exactly one member, found 2",
        ),
        (
            matcher(
                r#"{"outputPath": {"path": "a", "expected": "b", "comparator": "stringEquals"}}"#,
            ),
            "/W/acceptors/0/matcher/outputPath",
            "waiter `W` acceptor 0: unknown matcher member `outputPath`; \
             expected `output`, `inputOutput`, `success`, or `errorType`",
        ),
        (
            matcher(r#"{"success": "true"}"#),
            "/W/acceptors/0/matcher/success",
            "waiter `W` acceptor 0: expected `success` to be a boolean, found string",
        ),
        (
            matcher(r#"{"errorType": 1}"#),
            "/W/acceptors/0/matcher/errorType",
            "waiter `W` acceptor 0: expected `errorType` to be a string, found number",
        ),
        // Path matchers.
        (
            output(r#""thing.status""#),
            "/W/acceptors/0/matcher/output",
            "waiter `W` acceptor 0: expected `output` to be an object, found string",
        ),
        (
            matcher(r#"{"inputOutput": []}"#),
            "/W/acceptors/0/matcher/inputOutput",
            "waiter `W` acceptor 0: expected `inputOutput` to be an object, found array",
        ),
        (
            output(r#"{"expected": "b", "comparator": "stringEquals"}"#),
            "/W/acceptors/0/matcher/output",
            "waiter `W` acceptor 0: missing required member `path`",
        ),
        (
            output(r#"{"path": "a", "comparator": "stringEquals"}"#),
            "/W/acceptors/0/matcher/output",
            "waiter `W` acceptor 0: missing required member `expected`",
        ),
        (
            output(r#"{"path": "a", "expected": "b"}"#),
            "/W/acceptors/0/matcher/output",
            "waiter `W` acceptor 0: missing required member `comparator`",
        ),
        (
            output(r#"{"path": 1, "expected": "b", "comparator": "stringEquals"}"#),
            "/W/acceptors/0/matcher/output/path",
            "waiter `W` acceptor 0: expected `path` to be a string, found number",
        ),
        (
            output(r#"{"path": "a", "expected": true, "comparator": "booleanEquals"}"#),
            "/W/acceptors/0/matcher/output/expected",
            "waiter `W` acceptor 0: expected `expected` to be a string, found boolean",
        ),
        (
            output(r#"{"path": "a", "expected": "b", "comparator": "equals"}"#),
            "/W/acceptors/0/matcher/output/comparator",
            "waiter `W` acceptor 0: unknown path comparator `equals`; expected `stringEquals`, \
             `booleanEquals`, `allStringEquals`, or `anyStringEquals`",
        ),
    ];
    for (value, pointer, message) in cases {
        let err = decode(&value).unwrap_err();
        assert_eq!(err.trait_id(), Some(&WAITABLE), "{value}");
        assert_eq!(
            err.source_location().pointer(),
            Some(format!("{TRAIT}{pointer}").as_str()),
            "{value}"
        );
        assert_eq!(err.message(), message, "{value}");
    }
}

#[test]
fn decode_errors_display_the_trait_and_location() {
    let err = decode(&matcher("{}")).unwrap_err();
    let location = err.source_location();
    let (line, column) = (location.line().unwrap(), location.column().unwrap());
    assert_eq!(
        err.to_string(),
        format!(
            "m.json:{line}:{column}: invalid `smithy.waiters#waitable` trait value: \
             waiter `W` acceptor 0: expected `matcher` to set exactly one member, found 0 \
             (at {TRAIT}/W/acceptors/0/matcher)"
        )
    );
    // Values created in code have no location, so the message names the waiter and acceptor.
    let unlocated = Waitable::from_node(&Node::from(aws_smithy_lang::NodeObject::from_iter([(
        "W",
        Node::from(aws_smithy_lang::NodeObject::from_iter([(
            "acceptors",
            Node::from(vec![Node::from("success")]),
        )])),
    )])))
    .unwrap_err();
    assert!(unlocated.source_location().is_none());
    assert_eq!(
        unlocated.message(),
        "waiter `W` acceptor 0: expected an object, found string"
    );
}

#[test]
fn public_types_are_send_sync_and_clone() {
    fn assert<T: Clone + Send + Sync + 'static>() {}
    assert::<Waitable>();
    assert::<Waiter>();
    assert::<Acceptor>();
    assert::<AcceptorState>();
    assert::<Matcher>();
    assert::<MatcherKind>();
    assert::<PathMatcher>();
    assert::<PathComparator>();
    fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
    assert_error::<TraitDecodeError>();
}

/// Every `@waitable` trait in `aws/sdk/aws-models` decodes, round-trips, and has expressions
/// that parse, and the validator finds no errors. The corpus covers only part of the
/// specification, so this is a regression check; `java_fixtures.rs` checks agreement with
/// the reference implementation.
#[test]
fn corpus_waiters_decode_and_validate() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../aws/sdk/aws-models");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    let (mut models, mut operations, mut waiters, mut acceptors, mut warnings) = (0, 0, 0, 0, 0);
    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        if !text.contains(WAITABLE.as_str()) {
            continue;
        }
        let name = path.display();
        models += 1;
        let validated = Model::assembler()
            .add_json_str(name.to_string(), text)
            .add_validator(aws_smithy_lang::waiters::validator())
            .assemble();
        let errors: Vec<_> = validated
            .diagnostics()
            .errors()
            .map(|d| d.to_string())
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:#?}");
        warnings += validated
            .diagnostics()
            .iter()
            .filter(|d| d.validator() == Some("Waiters"))
            .count();
        let model = validated.into_result().unwrap();

        for operation in model.shapes_with_trait(&WAITABLE) {
            let id = operation.id();
            let waitable = operation
                .get_trait_as::<Waitable>()
                .unwrap_or_else(|e| panic!("{name}: {id}: {e}"))
                .unwrap();
            assert_eq!(
                Waitable::from_node(&waitable.to_node()).unwrap(),
                waitable,
                "{name}: {id}"
            );
            operations += 1;
            for (waiter_name, waiter) in waitable.waiters() {
                waiters += 1;
                for acceptor in waiter.acceptors() {
                    acceptors += 1;
                    let path = match acceptor.matcher().kind() {
                        MatcherKind::Output(matcher) | MatcherKind::InputOutput(matcher) => {
                            Some(matcher.path())
                        }
                        _ => None,
                    };
                    for expression in path.into_iter().chain(acceptor.message()) {
                        jmespath::parse(expression)
                            .unwrap_or_else(|e| panic!("{name}: {id} waiter `{waiter_name}`: {e}"));
                    }
                }
            }
        }
    }
    assert!(acceptors > 0, "no waiters in {}", dir.display());
    // Logged as a baseline; the corpus changes over time, so this is not asserted.
    println!(
        "corpus: {models} models, {operations} operations, {waiters} waiters, \
         {acceptors} acceptors, {warnings} warnings"
    );
}
