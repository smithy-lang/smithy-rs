/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! `StructuralV1` rules: the assembler's critical checks and the built-in validators.

use crate::diagnostic::DiagnosticCode::{self, *};
use crate::model::Model;
use crate::prelude;
use crate::ModelAssembler;
use crate::ShapeId;

fn check_with(assembler: ModelAssembler, shapes: &str) -> Vec<(DiagnosticCode, String)> {
    let json = format!(r#"{{"smithy":"2.0","shapes":{{{shapes}}}}}"#);
    match assembler
        .add_json_str("t.json", json)
        .assemble()
        .into_result()
    {
        Ok(_) => Vec::new(),
        Err(err) => {
            assert!(err.is_invalid_model(), "{err}");
            err.diagnostics()
                .iter()
                .map(|d| {
                    let pointer = d.source_location().and_then(|l| l.pointer());
                    (d.code(), pointer.unwrap_or_default().to_owned())
                })
                .collect()
        }
    }
}

fn check(shapes: &str) -> Vec<(DiagnosticCode, String)> {
    check_with(ModelAssembler::new(), shapes)
}

fn one(code: DiagnosticCode, pointer: &str) -> Vec<(DiagnosticCode, String)> {
    vec![(code, pointer.to_owned())]
}

const STR: &str = r#""a#Str":{"type":"string"}"#;

#[test]
fn prelude_passes_structural_v1() {
    let validated = ModelAssembler::new()
        .disable_prelude()
        .require_trait_definitions(true)
        .add_shapes(prelude::shapes().values().cloned())
        .assemble();
    assert!(
        validated.diagnostics().is_empty(),
        "{:?}",
        validated.diagnostics()
    );
}

#[test]
fn valid_model() {
    let shapes = r#"
        "a#Svc":{"type":"service","operations":[{"target":"a#Op"}],"resources":[{"target":"a#R"}],
                 "errors":[{"target":"a#Err"}],"rename":{"a#Str":"Name","a#In":"Input2"}},
        "a#Op":{"type":"operation","input":{"target":"a#In"},"output":{"target":"smithy.api#Unit"},
                "errors":[{"target":"a#Err"}]},
        "a#In":{"type":"structure","members":{"s":{"target":"a#Str"},"m":{"target":"a#M"}}},
        "a#Err":{"type":"structure","traits":{"smithy.api#error":"server"}},
        "a#Str":{"type":"string","traits":{"smithy.api#documentation":"d","a#undefined":{}}},
        "a#M":{"type":"map","key":{"target":"a#E"},"value":{"target":"a#U"}},
        "a#E":{"type":"enum","members":{"A":{"target":"smithy.api#Unit"},
               "B":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"b"}}}},
        "a#I":{"type":"intEnum","members":{"A":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":-1}}}},
        "a#U":{"type":"union","members":{"x":{"target":"smithy.api#Unit"},"y":{"target":"a#In"}}},
        "a#R":{"type":"resource","identifiers":{"id":{"target":"a#Str"}},"properties":{"p":{"target":"a#In"}},
               "read":{"target":"a#Op"},"operations":[{"target":"a#Op"}],
               "collectionOperations":[{"target":"a#Op"}],"resources":[{"target":"a#Child"}]},
        "a#Child":{"type":"resource"},
        "a#t":{"type":"structure","traits":{"smithy.api#trait":{}}},
        "a#Tagged":{"type":"string","traits":{"a#t":{}}}
    "#;
    assert_eq!(check(shapes), []);
}

#[test]
fn underscore_digit_identifiers_are_valid_everywhere() {
    // `IdentifierStart = (1*"_" (ALPHA / DIGIT)) / ALPHA`: namespaces, shape names, member
    // names, resource identifier/property names, and rename values may all be `_1`-style.
    let json = r#"{"smithy":"2.0","shapes":{
        "_1.a#_2":{"type":"structure","members":{"_3":{"target":"smithy.api#String"},
                   "__4x":{"target":"smithy.api#Integer"}}},
        "_1.a#Svc":{"type":"service","resources":[{"target":"_1.a#R"}],"rename":{"_1.a#_2":"_5"}},
        "_1.a#R":{"type":"resource","identifiers":{"_6":{"target":"smithy.api#String"}},
                  "properties":{"_7":{"target":"_1.a#_2"}}}
    }}"#;
    let model = Model::from_json_str("t.json", json).unwrap();
    let shape = model
        .expect_shape(&ShapeId::from_static("_1.a#_2$_3"))
        .unwrap();
    assert_eq!(shape.id().member(), Some("_3"));
}

#[test]
fn unresolved_and_member_references() {
    assert_eq!(
        check(r#""a#S":{"type":"structure","members":{"m":{"target":"a#Missing"}}}"#),
        one(UnresolvedReference, "/shapes/a#S/members/m/target")
    );
    assert_eq!(
        check(&format!(
            r#"{STR},"a#S":{{"type":"structure","members":{{"m":{{"target":"a#Str"}}}}}},
               "a#L":{{"type":"list","member":{{"target":"a#S$m"}}}}"#
        )),
        one(InvalidTarget, "/shapes/a#L/member/target")
    );
    assert_eq!(
        check_with(
            ModelAssembler::new().disable_prelude(),
            r#""a#L":{"type":"list","member":{"target":"smithy.api#String"}}"#
        ),
        one(UnresolvedReference, "/shapes/a#L/member/target")
    );
}

#[test]
fn members_target_data_shapes() {
    assert_eq!(
        check(r#""a#Op":{"type":"operation"},"a#L":{"type":"list","member":{"target":"a#Op"}}"#),
        one(InvalidTarget, "/shapes/a#L/member/target")
    );
    assert_eq!(
        check(
            r#""a#t":{"type":"structure","traits":{"smithy.api#trait":{}}},
               "a#L":{"type":"list","member":{"target":"a#t"}}"#
        ),
        one(InvalidTarget, "/shapes/a#L/member/target")
    );
}

#[test]
fn trait_definitions() {
    let shapes = format!(
        r#"{STR},"a#S":{{"type":"structure","members":{{"m":{{"target":"a#Str","traits":{{"a#Str":{{}}}}}}}},
                 "traits":{{"a#Str":{{}}}}}}"#
    );
    assert_eq!(
        check(&shapes),
        [
            (InvalidTrait, "/shapes/a#S/traits/a#Str".to_owned()),
            (
                InvalidTrait,
                "/shapes/a#S/members/m/traits/a#Str".to_owned()
            ),
        ]
    );
    let unknown = r#""a#S":{"type":"string","traits":{"x#unknown":{},"smithy.api#sensitive":{}}}"#;
    assert_eq!(check(unknown), []);
    assert_eq!(
        check_with(
            ModelAssembler::new().require_trait_definitions(true),
            unknown
        ),
        one(InvalidTrait, "/shapes/a#S/traits/x#unknown")
    );
}

#[test]
fn ordinary_references_cannot_target_trait_definitions() {
    // Each trait definition has an otherwise-valid kind for the position referencing it.
    let defs = r#"
        "a#TStr":{"type":"string","traits":{"smithy.api#trait":{}}},
        "a#TStruct":{"type":"structure","traits":{"smithy.api#trait":{}}},
        "a#TErr":{"type":"structure","traits":{"smithy.api#trait":{},"smithy.api#error":"client"}},
        "a#TOp":{"type":"operation","traits":{"smithy.api#trait":{}}},
        "a#TRes":{"type":"resource","traits":{"smithy.api#trait":{}}}
    "#;
    let cases = [
        (
            r#""a#L":{"type":"list","member":{"target":"a#TStr"}}"#,
            "/shapes/a#L/member/target",
        ),
        (
            r#""a#Op":{"type":"operation","input":{"target":"a#TStruct"}}"#,
            "/shapes/a#Op/input/target",
        ),
        (
            r#""a#Op":{"type":"operation","output":{"target":"a#TStruct"}}"#,
            "/shapes/a#Op/output/target",
        ),
        (
            r#""a#Op":{"type":"operation","errors":[{"target":"a#TErr"}]}"#,
            "/shapes/a#Op/errors/0/target",
        ),
        (
            r#""a#Svc":{"type":"service","errors":[{"target":"a#TErr"}]}"#,
            "/shapes/a#Svc/errors/0/target",
        ),
        (
            r#""a#Svc":{"type":"service","operations":[{"target":"a#TOp"}]}"#,
            "/shapes/a#Svc/operations/0/target",
        ),
        (
            r#""a#Svc":{"type":"service","resources":[{"target":"a#TRes"}]}"#,
            "/shapes/a#Svc/resources/0/target",
        ),
        (
            r#""a#R":{"type":"resource","identifiers":{"id":{"target":"a#TStr"}}}"#,
            "/shapes/a#R/identifiers/id/target",
        ),
        (
            r#""a#R":{"type":"resource","properties":{"p":{"target":"a#TStruct"}}}"#,
            "/shapes/a#R/properties/p/target",
        ),
        (
            r#""a#R":{"type":"resource","read":{"target":"a#TOp"}}"#,
            "/shapes/a#R/read/target",
        ),
        (
            r#""a#R":{"type":"resource","operations":[{"target":"a#TOp"}]}"#,
            "/shapes/a#R/operations/0/target",
        ),
        (
            r#""a#R":{"type":"resource","resources":[{"target":"a#TRes"}]}"#,
            "/shapes/a#R/resources/0/target",
        ),
    ];
    for (shape, pointer) in cases {
        assert_eq!(
            check(&format!("{defs},{shape}")),
            one(InvalidTarget, pointer),
            "{shape}"
        );
    }
    // Applying the definitions is the permitted way to reference them.
    let applied = r#""a#S":{"type":"string","traits":{"a#TStr":"x","a#TStruct":{}}}"#;
    assert_eq!(check(&format!("{defs},{applied}")), []);
}

#[test]
fn map_keys() {
    assert_eq!(
        check(
            r#""a#M":{"type":"map","key":{"target":"smithy.api#Integer"},"value":{"target":"smithy.api#Integer"}}"#
        ),
        one(InvalidTarget, "/shapes/a#M/key/target")
    );
    assert_eq!(
        check(
            r#""a#M":{"type":"map","key":{"target":"smithy.api#String"},"value":{"target":"smithy.api#String"}}"#
        ),
        []
    );
}

#[test]
fn case_conflicts() {
    let err = Model::from_json_str(
        "t",
        r#"{"smithy":"2.0","shapes":{"a#S":{"type":"structure","members":{
            "foo":{"target":"smithy.api#String"},"Foo":{"target":"smithy.api#String"}}}}}"#,
    )
    .unwrap_err();
    let d = err.diagnostics().iter().next().unwrap();
    assert_eq!(d.code(), CaseConflict);
    assert_eq!(d.validator(), Some("CaseConflict"));
    assert_eq!(d.shape_id().unwrap(), "a#S$Foo");
    assert_eq!(
        d.source_location().unwrap().pointer(),
        Some("/shapes/a#S/members/Foo")
    );
    assert_eq!(d.related()[0].pointer(), Some("/shapes/a#S/members/foo"));

    assert_eq!(
        check(r#""a#Foo":{"type":"string"},"a#foo":{"type":"string"}"#),
        one(CaseConflict, "/shapes/a#foo")
    );
    // A different-case namespace passes the loader's prelude check but conflicts here.
    assert_eq!(
        check(r#""smithy.API#string":{"type":"string"}"#),
        one(CaseConflict, "/shapes/smithy.API#string")
    );
    assert_eq!(
        check(
            r#""a#U":{"type":"union","members":{"a":{"target":"smithy.api#String"},"A":{"target":"smithy.api#String"}}}"#
        ),
        one(CaseConflict, "/shapes/a#U/members/A")
    );
}

#[test]
fn unions_and_enums() {
    assert_eq!(
        check(r#""a#U":{"type":"union"}"#),
        one(InvalidShape, "/shapes/a#U")
    );
    assert_eq!(
        check(r#""a#E":{"type":"enum","members":{}}"#),
        one(InvalidShape, "/shapes/a#E")
    );
    assert_eq!(
        check(r#""a#E":{"type":"intEnum"}"#),
        one(InvalidShape, "/shapes/a#E")
    );
    assert_eq!(
        check(r#""a#E":{"type":"enum","members":{"A":{"target":"smithy.api#String"}}}"#),
        one(InvalidTarget, "/shapes/a#E/members/A/target")
    );
    assert_eq!(
        check(
            r#""a#E":{"type":"enum","members":{"A":{"target":"smithy.api#Unit"},
            "B":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"A"}}}}"#
        ),
        one(InvalidShape, "/shapes/a#E/members/B")
    );
    for bad in [r#"1"#, r#""""#] {
        assert_eq!(
            check(&format!(
                r#""a#E":{{"type":"enum","members":{{"A":{{"target":"smithy.api#Unit","traits":{{"smithy.api#enumValue":{bad}}}}}}}}}"#
            )),
            one(
                InvalidTrait,
                "/shapes/a#E/members/A/traits/smithy.api#enumValue"
            )
        );
    }
    assert_eq!(
        check(r#""a#I":{"type":"intEnum","members":{"A":{"target":"smithy.api#Unit"}}}"#),
        one(InvalidShape, "/shapes/a#I/members/A")
    );
    for bad in ["1.5", "\"1\"", "2147483648"] {
        assert_eq!(
            check(&format!(
                r#""a#I":{{"type":"intEnum","members":{{"A":{{"target":"smithy.api#Unit","traits":{{"smithy.api#enumValue":{bad}}}}}}}}}"#
            )),
            one(
                InvalidTrait,
                "/shapes/a#I/members/A/traits/smithy.api#enumValue"
            ),
            "{bad}"
        );
    }
    assert_eq!(
        check(
            r#""a#I":{"type":"intEnum","members":{
            "A":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":1}},
            "B":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":1}}}}"#
        ),
        one(InvalidShape, "/shapes/a#I/members/B")
    );
}

#[test]
fn unit_targets() {
    assert_eq!(
        check(r#""a#S":{"type":"structure","members":{"m":{"target":"smithy.api#Unit"}}}"#),
        one(InvalidTarget, "/shapes/a#S/members/m/target")
    );
    assert_eq!(
        check(r#""a#L":{"type":"list","member":{"target":"smithy.api#Unit"}}"#),
        one(InvalidTarget, "/shapes/a#L/member/target")
    );
    assert_eq!(
        check(r#""a#R":{"type":"resource","properties":{"p":{"target":"smithy.api#Unit"}}}"#),
        one(InvalidTarget, "/shapes/a#R/properties/p/target")
    );
}

#[test]
fn services() {
    let shapes = format!(
        r#"{STR},"a#S":{{"type":"structure"}},
           "a#Svc":{{"type":"service","operations":[{{"target":"a#S"}}],"resources":[{{"target":"a#S"}}],
                   "errors":[{{"target":"a#S"}},{{"target":"a#Str"}}]}}"#
    );
    // Wrong target types are critical: validators do not run.
    assert_eq!(
        check(&shapes),
        [
            (
                InvalidTarget,
                "/shapes/a#Svc/operations/0/target".to_owned()
            ),
            (InvalidTarget, "/shapes/a#Svc/resources/0/target".to_owned()),
            (InvalidTarget, "/shapes/a#Svc/errors/1/target".to_owned()),
        ]
    );
    let shapes = format!(
        r#"{STR},"a#S":{{"type":"structure"}},
           "a#Svc":{{"type":"service","errors":[{{"target":"a#S"}}],
                   "rename":{{"a#Missing":"X","a#S":"not-valid","a#Str":"Dup"}}}},
           "a#Svc2":{{"type":"service","rename":{{"a#S":"Dup","a#Str":"dup"}}}}"#
    );
    assert_eq!(
        check(&shapes),
        [
            (InvalidTarget, "/shapes/a#Svc/errors/0/target".to_owned()),
            (
                UnresolvedReference,
                "/shapes/a#Svc/rename/a#Missing".to_owned()
            ),
            (InvalidProperty, "/shapes/a#Svc/rename/a#S".to_owned()),
            (CaseConflict, "/shapes/a#Svc2/rename/a#Str".to_owned()),
        ]
    );
}

#[test]
fn operations() {
    let shapes = format!(
        r#"{STR},"a#S":{{"type":"structure"}},
           "a#Op":{{"type":"operation","input":{{"target":"a#Str"}},"output":{{"target":"a#Op"}},
                   "errors":[{{"target":"a#S"}}]}}"#
    );
    assert_eq!(
        check(&shapes),
        [
            (InvalidTarget, "/shapes/a#Op/input/target".to_owned()),
            (InvalidTarget, "/shapes/a#Op/output/target".to_owned()),
        ]
    );
    // `errors` must target `@error` structures (checked once targets are valid).
    assert_eq!(
        check(
            r#""a#S":{"type":"structure"},"a#Op":{"type":"operation","errors":[{"target":"a#S"}]}"#
        ),
        one(InvalidTarget, "/shapes/a#Op/errors/0/target")
    );
}

#[test]
fn omitted_operation_io_requires_unit() {
    assert_eq!(
        check_with(
            ModelAssembler::new().disable_prelude(),
            r#""a#Op":{"type":"operation"}"#
        ),
        [
            (UnresolvedReference, "/shapes/a#Op".to_owned()),
            (UnresolvedReference, "/shapes/a#Op".to_owned()),
        ]
    );
    // Defining Unit locally makes the normalized reference resolve.
    let unit = r#""smithy.api#Unit":{"type":"structure","traits":{"smithy.api#unitType":{}}},
                  "a#Op":{"type":"operation"}"#;
    assert_eq!(
        check_with(ModelAssembler::new().disable_prelude(), unit),
        []
    );
    assert_eq!(check(r#""a#Op":{"type":"operation"}"#), []);
}

#[test]
fn resources() {
    let shapes = r#""a#S":{"type":"structure"},"a#Op":{"type":"operation"},
        "a#R":{"type":"resource","identifiers":{"id":{"target":"smithy.api#Integer"}},
               "properties":{"p":{"target":"a#Op"}},"read":{"target":"a#S"},
               "operations":[{"target":"a#S"}],"collectionOperations":[{"target":"a#S"}],
               "resources":[{"target":"a#Op"}]}"#;
    assert_eq!(
        check(shapes),
        [
            (
                InvalidTarget,
                "/shapes/a#R/identifiers/id/target".to_owned()
            ),
            (InvalidTarget, "/shapes/a#R/properties/p/target".to_owned()),
            (InvalidTarget, "/shapes/a#R/read/target".to_owned()),
            (InvalidTarget, "/shapes/a#R/operations/0/target".to_owned()),
            (
                InvalidTarget,
                "/shapes/a#R/collectionOperations/0/target".to_owned()
            ),
            (InvalidTarget, "/shapes/a#R/resources/0/target".to_owned()),
        ]
    );
}

#[test]
fn resource_cycles() {
    assert_eq!(
        check(
            r#""a#A":{"type":"resource","resources":[{"target":"a#B"}]},
               "a#B":{"type":"resource","resources":[{"target":"a#C"}]},
               "a#C":{"type":"resource","resources":[{"target":"a#A"}]}"#
        ),
        one(InvalidShape, "/shapes/a#C/resources/0/target")
    );
    let err = Model::from_json_str(
        "t",
        r#"{"smithy":"2.0","shapes":{"a#A":{"type":"resource","resources":[{"target":"a#A"}]}}}"#,
    )
    .unwrap_err();
    let message = err
        .diagnostics()
        .iter()
        .next()
        .unwrap()
        .message()
        .to_owned();
    assert_eq!(message, "resource containment cycle: a#A -> a#A");
    // A diamond is not a cycle.
    assert_eq!(
        check(
            r#""a#A":{"type":"resource","resources":[{"target":"a#B"},{"target":"a#C"}]},
               "a#B":{"type":"resource","resources":[{"target":"a#D"}]},
               "a#C":{"type":"resource","resources":[{"target":"a#D"}]},
               "a#D":{"type":"resource"}"#
        ),
        []
    );
}
