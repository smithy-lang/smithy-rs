/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Validation of `smithy.waiters#waitable` traits.

use aws_smithy_lang::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use aws_smithy_lang::{waiters, Model, ShapeId};
use serde_json::{json, Value};

/// The JSON Pointer of the trait value of `ex#GetThing`.
const TRAIT: &str = "/shapes/ex#GetThing/traits/smithy.waiters#waitable";

/// A service with `ex#GetThing`, whose trait is `waitable`, and `ex#ListThings`, bound
/// through a resource. `shapes` are added to the model.
fn model_with(waitable: Value, shapes: Value) -> Value {
    let mut model = json!({
        "smithy": "2.0",
        "shapes": {
            "ex#Service": {
                "type": "service",
                "version": "1",
                "operations": [{"target": "ex#GetThing"}],
                "resources": [{"target": "ex#Thing"}]
            },
            "ex#Thing": {"type": "resource", "operations": [{"target": "ex#ListThings"}]},
            "ex#ListThings": {"type": "operation"},
            "ex#GetThing": {
                "type": "operation",
                "input": {"target": "ex#GetThingInput"},
                "output": {"target": "ex#GetThingOutput"},
                "errors": [{"target": "ex#NotFound"}],
                "traits": {"smithy.waiters#waitable": waitable}
            },
            "ex#GetThingInput": {
                "type": "structure",
                "members": {"name": {"target": "smithy.api#String"}}
            },
            "ex#GetThingOutput": {
                "type": "structure",
                "members": {
                    "name": {"target": "smithy.api#String"},
                    "status": {"target": "ex#Status"},
                    "ready": {"target": "smithy.api#Boolean"},
                    "count": {"target": "smithy.api#Integer"},
                    "reasons": {"target": "ex#Strings"},
                    "items": {"target": "ex#Items"},
                    "doc": {"target": "smithy.api#Document"}
                }
            },
            "ex#Status": {
                "type": "enum",
                "members": {
                    "READY": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "ready"}},
                    "FAILED": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "failed"}}
                }
            },
            "ex#Strings": {"type": "list", "member": {"target": "smithy.api#String"}},
            "ex#Items": {"type": "list", "member": {"target": "ex#Item"}},
            "ex#Item": {"type": "structure", "members": {"id": {"target": "smithy.api#String"}}},
            "ex#NotFound": {"type": "structure", "traits": {"smithy.api#error": "client"}}
        }
    });
    let all = model["shapes"].as_object_mut().unwrap();
    for (id, shape) in shapes.as_object().unwrap() {
        all.insert(id.clone(), shape.clone());
    }
    model
}

fn model(waitable: Value) -> Value {
    model_with(waitable, json!({}))
}

/// A waitable trait with one waiter, `W`, with `acceptors`.
fn waiter(acceptors: Value) -> Value {
    json!({"W": {"acceptors": acceptors}})
}

fn success() -> Value {
    json!({"state": "success", "matcher": {"success": true}})
}

fn output(path: &str, expected: &str, comparator: &str) -> Value {
    json!({"output": {"path": path, "expected": expected, "comparator": comparator}})
}

/// The diagnostics reported by the waiter validator.
fn validate(model: Value) -> Vec<Diagnostic> {
    Model::assembler()
        .add_json_str("model.json", model.to_string())
        .add_validator(waiters::validator())
        .assemble()
        .diagnostics()
        .iter()
        .filter(|d| d.validator() == Some("Waiters"))
        .cloned()
        .collect()
}

/// A diagnostic `expect` requires.
struct Expected {
    severity: Severity,
    code: DiagnosticCode,
    message: String,
    /// The JSON Pointer, relative to [`TRAIT`].
    pointer: String,
}

fn error(message: &str, pointer: &str) -> Expected {
    Expected {
        severity: Severity::Error,
        code: DiagnosticCode::InvalidTrait,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

fn warning(message: &str, pointer: &str) -> Expected {
    Expected {
        severity: Severity::Warning,
        ..error(message, pointer)
    }
}

fn unknown_property(message: &str, pointer: &str) -> Expected {
    Expected {
        code: DiagnosticCode::UnknownProperty,
        ..warning(message, pointer)
    }
}

/// Asserts that validating `model` reports exactly `expected`, in order. Every diagnostic
/// must be about `ex#GetThing`.
fn expect(model: Value, expected: &[Expected]) {
    let diagnostics = validate(model);
    let describe = |severity, code, message: &str, pointer: &str| {
        format!("{severity:?} {code:?} {pointer}: {message}")
    };
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|d| {
            let pointer = d.source_location().and_then(|l| l.pointer()).unwrap_or("");
            let pointer = pointer.strip_prefix(TRAIT).unwrap_or(pointer);
            describe(d.severity(), d.code(), d.message(), pointer)
        })
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .map(|e| describe(e.severity, e.code, &e.message, &e.pointer))
        .collect();
    assert_eq!(actual, expected);
    for diagnostic in &diagnostics {
        assert_eq!(
            diagnostic.shape_id(),
            Some(&ShapeId::from_static("ex#GetThing")),
            "{diagnostic}"
        );
    }
}

#[test]
fn valid_waiters_have_no_diagnostics() {
    let waitable = json!({
        "ThingReady": {
            "documentation": "Waits until the thing is ready.",
            "acceptors": [
                {"state": "success", "matcher": output("status", "ready", "stringEquals")},
                {"state": "success", "matcher": output("ready", "true", "booleanEquals")},
                {"state": "success", "matcher": {"inputOutput": {
                    "path": "input.name == output.name", "expected": "true", "comparator": "booleanEquals"
                }}},
                {
                    "state": "failure",
                    "matcher": output("items[].id", "bad", "anyStringEquals"),
                    "message": "reasons"
                },
                {
                    "state": "failure",
                    "matcher": output("reasons", "x", "allStringEquals"),
                    "message": "join(', ', reasons)"
                },
                {
                    "state": "failure",
                    "matcher": output("length(items[?id == 'x']) > `0`", "true", "booleanEquals"),
                    "message": "doc.anything"
                },
                {"state": "retry", "matcher": {"errorType": "NotFound"}},
                {"state": "retry", "matcher": {"errorType": "ex#NotFound"}},
                {"state": "failure", "matcher": {"success": false}}
            ],
            "minDelay": 5,
            "maxDelay": 5,
            "deprecated": true,
            "tags": ["slow"]
        }
    });
    expect(model(waitable), &[]);
}

#[test]
fn reports_malformed_trait_values() {
    expect(
        model(json!({"W": {"acceptors": []}})),
        &[error(
            "invalid `smithy.waiters#waitable` trait value: waiter `W`: expected at least one \
             acceptor",
            "/W/acceptors",
        )],
    );
}

#[test]
fn requires_an_operation_without_event_streams() {
    let shapes = json!({
        "ex#Value": {
            "type": "structure",
            "traits": {"smithy.waiters#waitable": waiter(json!([success()]))}
        }
    });
    let diagnostics = validate(model_with(waiter(json!([success()])), shapes));
    let [diagnostic] = &diagnostics[..] else {
        panic!("expected one diagnostic, found {diagnostics:#?}");
    };
    assert_eq!(diagnostic.severity(), Severity::Error);
    assert_eq!(
        diagnostic.message(),
        "`smithy.waiters#waitable` can only be applied to operations, but `ex#Value` is a \
         structure"
    );
    assert_eq!(
        diagnostic.source_location().and_then(|l| l.pointer()),
        Some("/shapes/ex#Value/traits/smithy.waiters#waitable")
    );

    let mut streaming = model(waiter(json!([success()])));
    let shapes = streaming["shapes"].as_object_mut().unwrap();
    shapes.insert(
        "ex#Events".into(),
        json!({
            "type": "union",
            "members": {"item": {"target": "ex#Item"}},
            "traits": {"smithy.api#streaming": {}}
        }),
    );
    shapes["ex#GetThingOutput"]["members"]["events"] = json!({"target": "ex#Events"});
    expect(
        streaming,
        &[error(
            "`smithy.waiters#waitable` cannot be applied to an operation with an event \
             stream, but output member `ex#GetThingOutput$events` is an event stream",
            "",
        )],
    );
}

#[test]
fn checks_delays_and_acceptor_states() {
    expect(
        model(json!({
            "W": {
                "acceptors": [{"state": "retry", "matcher": {"success": false}}],
                "minDelay": 10,
                "maxDelay": 5
            }
        })),
        &[
            error(
                "waiter `W`: `minDelay` of 10 is greater than `maxDelay` of 5",
                "/W",
            ),
            error("waiter `W`: no acceptor has the `success` state", "/W"),
        ],
    );

    // Only failure acceptors may have a message.
    expect(
        model(waiter(json!([
            {"state": "success", "matcher": {"success": true}, "message": "name"},
            {"state": "retry", "matcher": {"success": false}, "message": "name"}
        ]))),
        &[
            error(
                "waiter `W` acceptor 0: `message` is only allowed on acceptors with the \
                 `failure` state, but the state is `success`",
                "/W/acceptors/0/message",
            ),
            error(
                "waiter `W` acceptor 1: `message` is only allowed on acceptors with the \
                 `failure` state, but the state is `retry`",
                "/W/acceptors/1/message",
            ),
        ],
    );
}

#[test]
fn reports_invalid_expressions_at_the_string() {
    expect(
        model(waiter(json!([
            {"state": "success", "matcher": output("items[", "x", "stringEquals")},
            {"state": "failure", "matcher": {"success": false}, "message": "length(name"}
        ]))),
        &[
            error(
                "waiter `W` acceptor 0: `path` \"items[\" is not valid JMESPath at column 7: \
                 Expected number, ':', or '*' -- found Eof",
                "/W/acceptors/0/matcher/output/path",
            ),
            error(
                "waiter `W` acceptor 1: `message` \"length(name\" is not valid JMESPath at \
                 column 12: Unexpected nud token -- found Eof",
                "/W/acceptors/1/message",
            ),
        ],
    );

    // Unknown functions and wrong arities are errors, as in the reference implementation.
    expect(
        model(waiter(json!([
            {"state": "success", "matcher": output("lenght(items) > `0`", "true", "booleanEquals")},
            {"state": "success", "matcher": output("length(items, name) > `0`", "true", "booleanEquals")}
        ]))),
        &[
            error(
                "waiter `W` acceptor 0: `path` \"lenght(items) > `0`\" at column 7: `lenght` \
                 is not a JMESPath function",
                "/W/acceptors/0/matcher/output/path",
            ),
            error(
                "waiter `W` acceptor 1: `path` \"length(items, name) > `0`\" at column 7: \
                 `length` takes 1 argument, but 2 were given",
                "/W/acceptors/1/matcher/output/path",
            ),
        ],
    );
}

#[test]
fn limits_the_size_of_expressions() {
    // 4 + 256 * 8 = 2052 bytes, just past the limit.
    let long = format!("name{}", " || name".repeat(256));
    let nested = format!("{}name{}", "(".repeat(33), ")".repeat(33));
    let diagnostics = validate(model(waiter(json!([
        {"state": "success", "matcher": output(&long, "x", "stringEquals")},
        {"state": "success", "matcher": output(&nested, "x", "stringEquals")}
    ]))));
    let summary: Vec<_> = diagnostics
        .iter()
        .map(|d| (d.severity(), d.code(), d.message()))
        .collect();
    assert_eq!(
        summary,
        [
            (
                Severity::Error,
                DiagnosticCode::ResourceLimit,
                "waiter `W` acceptor 0: `path` is 2052 bytes long, longer than the 2048 bytes \
                 this validator parses"
            ),
            (
                Severity::Error,
                DiagnosticCode::ResourceLimit,
                "waiter `W` acceptor 1: `path` has 33 nesting tokens (`(`, `[`, `{`, `!`, `&`, \
                 or `*`), more than the 32 this validator parses"
            ),
        ]
    );
}

#[test]
fn checks_message_types() {
    let failure = |message: &str| json!({"state": "failure", "matcher": {"success": false}, "message": message});
    expect(
        model(waiter(json!([
            success(),
            failure("name"),
            failure("reasons"),
            failure("items[].id"),
            failure("doc"),
            failure("count"),
            failure("items"),
            failure("nmae")
        ]))),
        &[
            error(
                "waiter `W` acceptor 5: `message` must evaluate to a `string` or an `array` of \
                 `string` values, but evaluates to a `number`",
                "/W/acceptors/5/message",
            ),
            error(
                "waiter `W` acceptor 6: `message` must evaluate to a `string` or an `array` of \
                 `string` values, but evaluates to an `array` of `object` values",
                "/W/acceptors/6/message",
            ),
            warning(
                "waiter `W` acceptor 7: `message` \"nmae\" at column 1: `nmae` is not a member \
                 of `ex#GetThingOutput`, so it is always null",
                "/W/acceptors/7/message",
            ),
            error(
                "waiter `W` acceptor 7: `message` must evaluate to a `string` or an `array` of \
                 `string` values, but evaluates to a `null`",
                "/W/acceptors/7/message",
            ),
        ],
    );
}

#[test]
fn checks_path_matchers() {
    expect(
        model(waiter(json!([
            {"state": "success", "matcher": output("ready", "yes", "booleanEquals")},
            {"state": "success", "matcher": output("count", "1", "stringEquals")},
            {"state": "success", "matcher": output("name", "true", "booleanEquals")},
            {"state": "success", "matcher": output("name", "x", "allStringEquals")},
            {"state": "success", "matcher": output("items[].ready", "x", "anyStringEquals")},
            {"state": "success", "matcher": output("doc.x", "x", "anyStringEquals")}
        ]))),
        &[
            error(
                "waiter `W` acceptor 0: a `booleanEquals` matcher must expect `true` or \
                 `false`, but expects `yes`",
                "/W/acceptors/0/matcher/output",
            ),
            warning(
                "waiter `W` acceptor 1: a `stringEquals` matcher compares a `string`, but \
                 `path` evaluates to a `number`",
                "/W/acceptors/1/matcher/output/path",
            ),
            warning(
                "waiter `W` acceptor 2: a `booleanEquals` matcher compares a `boolean`, but \
                 `path` evaluates to a `string`",
                "/W/acceptors/2/matcher/output/path",
            ),
            warning(
                "waiter `W` acceptor 3: an `allStringEquals` matcher compares an `array` of \
                 `string` values, but `path` evaluates to a `string`",
                "/W/acceptors/3/matcher/output/path",
            ),
            warning(
                "waiter `W` acceptor 4: `path` \"items[].ready\" at column 9: `ready` is not a \
                 member of `ex#Item`, so it is always null",
                "/W/acceptors/4/matcher/output/path",
            ),
        ],
    );
}

#[test]
fn evaluates_input_output_and_unit_shapes() {
    let input_output = |path: &str| {
        json!({"state": "success", "matcher": {"inputOutput": {
            "path": path, "expected": "x", "comparator": "stringEquals"
        }}})
    };
    expect(
        model(waiter(json!([
            input_output("input.name"),
            input_output("output.name"),
            input_output("name")
        ]))),
        &[
            warning(
                "waiter `W` acceptor 2: `path` \"name\" at column 1: `name` is not one of the \
                 fields `input`, `output`, so it is always null",
                "/W/acceptors/2/matcher/inputOutput/path",
            ),
            warning(
                "waiter `W` acceptor 2: a `stringEquals` matcher compares a `string`, but \
                 `path` evaluates to a `null`",
                "/W/acceptors/2/matcher/inputOutput/path",
            ),
        ],
    );

    // An operation without input or output is evaluated against empty structures.
    let mut unit = model(waiter(json!([input_output("input.name")])));
    let operation = unit["shapes"]["ex#GetThing"].as_object_mut().unwrap();
    operation.remove("input");
    operation.remove("output");
    expect(
        unit,
        &[
            warning(
                "waiter `W` acceptor 0: `path` \"input.name\" at column 7: `name` is not a \
                 member of `smithy.api#Unit`, so it is always null",
                "/W/acceptors/0/matcher/inputOutput/path",
            ),
            warning(
                "waiter `W` acceptor 0: a `stringEquals` matcher compares a `string`, but \
                 `path` evaluates to a `null`",
                "/W/acceptors/0/matcher/inputOutput/path",
            ),
        ],
    );
}

#[test]
fn warns_about_unmodeled_error_types() {
    expect(
        model(waiter(json!([
            success(),
            {"state": "retry", "matcher": {"errorType": "NotFound"}},
            {"state": "retry", "matcher": {"errorType": "ex#NotFound"}},
            {"state": "retry", "matcher": {"errorType": "ThrottlingException"}},
            {"state": "retry", "matcher": {"errorType": "other#NotFound"}}
        ]))),
        &[
            warning(
                "waiter `W` acceptor 3: `errorType` `ThrottlingException` is not an error of \
                 the operation, which lists `ex#NotFound`",
                "/W/acceptors/3/matcher/errorType",
            ),
            warning(
                "waiter `W` acceptor 4: `errorType` `other#NotFound` is not an error of the \
                 operation, which lists `ex#NotFound`",
                "/W/acceptors/4/matcher/errorType",
            ),
        ],
    );
}

#[test]
fn warns_about_ignored_properties() {
    expect(
        model(json!({
            "W": {
                "acceptors": [{
                    "state": "success",
                    "matcher": {"output": {
                        "path": "name", "expected": "x", "comparator": "stringEquals", "extra": 1
                    }},
                    "note": "n"
                }],
                "future": true
            }
        })),
        &[
            unknown_property(
                "waiter `W`: unknown waiter property `future` is ignored",
                "/W/future",
            ),
            unknown_property(
                "waiter `W` acceptor 0: unknown acceptor property `note` is ignored",
                "/W/acceptors/0/note",
            ),
            unknown_property(
                "waiter `W` acceptor 0: unknown path matcher property `extra` is ignored",
                "/W/acceptors/0/matcher/output/extra",
            ),
        ],
    );
}

/// `(severity, shape ID, message, pointer, related pointers)` of each diagnostic.
fn summarize(diagnostics: &[Diagnostic]) -> Vec<(Severity, String, String, String, Vec<String>)> {
    let pointer = |location: &aws_smithy_lang::SourceLocation| {
        location.pointer().unwrap_or_default().to_owned()
    };
    diagnostics
        .iter()
        .map(|d| {
            (
                d.severity(),
                d.shape_id().map(ToString::to_string).unwrap_or_default(),
                d.message().to_owned(),
                d.source_location().map(pointer).unwrap_or_default(),
                d.related().iter().map(pointer).collect(),
            )
        })
        .collect()
}

fn waitable(names: &[&str]) -> Value {
    names
        .iter()
        .map(|name| (name.to_string(), json!({"acceptors": [success()]})))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

#[test]
fn requires_case_insensitively_unique_names_in_a_service_closure() {
    // `ex#ListThings` is bound through a resource.
    let mut conflict = model(waitable(&["ThingReady"]));
    conflict["shapes"]["ex#ListThings"]["traits"] =
        json!({"smithy.waiters#waitable": waitable(&["THINGREADY"])});
    let list = "/shapes/ex#ListThings/traits/smithy.waiters#waitable";
    assert_eq!(
        summarize(&validate(conflict)),
        [
            (
                Severity::Error,
                "ex#GetThing".into(),
                "waiter `ThingReady` conflicts with `THINGREADY` of `ex#ListThings`: waiter \
                 names must be case-insensitively unique in the closure of service `ex#Service`"
                    .into(),
                format!("{TRAIT}/ThingReady"),
                vec![format!("{list}/THINGREADY")],
            ),
            (
                Severity::Error,
                "ex#ListThings".into(),
                "waiter `THINGREADY` conflicts with `ThingReady` of `ex#GetThing`: waiter \
                 names must be case-insensitively unique in the closure of service `ex#Service`"
                    .into(),
                format!("{list}/THINGREADY"),
                vec![format!("{TRAIT}/ThingReady")],
            ),
        ]
    );

    // The specification also forbids names of one operation that differ only by case.
    expect(
        model(waitable(&["ThingReady", "Thingready"])),
        &[
            error(
                "waiter `ThingReady` conflicts with `Thingready` of `ex#GetThing`: waiter names \
                 must be case-insensitively unique in the closure of service `ex#Service`",
                "/ThingReady",
            ),
            error(
                "waiter `Thingready` conflicts with `ThingReady` of `ex#GetThing`: waiter names \
                 must be case-insensitively unique in the closure of service `ex#Service`",
                "/Thingready",
            ),
        ],
    );

    // Operations outside the service's closure, or in another service's, do not conflict.
    let mut unrelated = model(waitable(&["ThingReady"]));
    let shapes = unrelated["shapes"].as_object_mut().unwrap();
    shapes.insert(
        "ex#Other".into(),
        json!({"type": "service", "version": "1", "operations": [{"target": "ex#Elsewhere"}]}),
    );
    shapes.insert(
        "ex#Elsewhere".into(),
        json!({"type": "operation", "traits": {"smithy.waiters#waitable": waitable(&["ThingReady"])}}),
    );
    shapes.insert(
        "ex#Unbound".into(),
        json!({"type": "operation", "traits": {"smithy.waiters#waitable": waitable(&["ThingReady"])}}),
    );
    assert_eq!(validate(unrelated), []);
}
