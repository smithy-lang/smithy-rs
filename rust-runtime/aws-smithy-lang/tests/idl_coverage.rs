/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Broader Smithy IDL coverage: resolution across inputs, sugar lowering, exact locations,
//! limits, robustness, and the repository's own `.smithy` models.

use aws_smithy_lang::diagnostic::{DiagnosticCode, Severity};
use aws_smithy_lang::shape::SimpleShape;
use aws_smithy_lang::{AstWriter, LoadError, Model, ShapeId, ShapeType};
use proptest::prelude::*;
use std::path::{Path, PathBuf};

fn id(value: &str) -> ShapeId {
    ShapeId::new(value).unwrap()
}

fn idl(input: &str) -> Model {
    Model::from_idl_str("model.smithy", input)
        .unwrap_or_else(|e| panic!("{e}: {:?}", e.diagnostics()))
}

fn idl_err(input: &str) -> LoadError {
    Model::from_idl_str("model.smithy", input).expect_err("expected an error")
}

fn codes(err: &LoadError) -> Vec<DiagnosticCode> {
    err.diagnostics().iter().map(|d| d.code()).collect()
}

fn target(model: &Model, member: &str) -> String {
    let (root, name) = member.split_once('$').unwrap();
    let shape = model.expect_shape(&id(root)).unwrap();
    let member = shape.members().get(name).unwrap();
    member.target().to_string()
}

fn json(model: &Model) -> String {
    AstWriter::new().canonical(true).to_string(model).unwrap()
}

// --- Relative shape ID resolution --------------------------------------------------------

#[test]
fn resolution_order_is_import_then_namespace_then_prelude() {
    let other = "namespace other\nstring String\nstring Imported\n";
    let main = "namespace ex
use other#String

structure S {
    // Imported: wins over a same-namespace shape and the prelude.
    a: String
    // Same namespace, declared later in this file.
    b: Later
    // Same namespace, declared only by the JSON input.
    c: FromJson
    // Same namespace, declared in code.
    d: FromCode
    // Prelude.
    e: Integer
    // Same namespace, declared by an existing model; shadows the prelude `Boolean`.
    f: Boolean
    // Unresolved names keep the current namespace.
    g: Nowhere
}

string Later
";
    let json = r#"{"smithy": "2.0", "shapes": {"ex#FromJson": {"type": "blob"}}}"#;
    let existing = Model::from_json_str(
        "existing.json",
        r#"{"smithy": "2.0", "shapes": {"ex#Boolean": {"type": "string"}}}"#,
    )
    .unwrap();
    let err = Model::assembler()
        .add_idl_str("main.smithy", main)
        .add_idl_str("other.smithy", other)
        .add_json_str("extra.json", json)
        .add_shape(
            SimpleShape::builder(ShapeType::Long, id("ex#FromCode"))
                .build()
                .unwrap(),
        )
        .add_model(&existing)
        .assemble()
        .into_result()
        .unwrap_err();
    // Only the deliberately unresolved member fails, and it was qualified into `ex`.
    let messages: Vec<_> = err
        .diagnostics()
        .iter()
        .map(|d| d.message().to_owned())
        .collect();
    assert_eq!(
        codes(&err),
        [DiagnosticCode::UnresolvedReference],
        "{messages:?}"
    );
    assert!(messages[0].contains("ex#Nowhere"), "{messages:?}");

    let model = Model::assembler()
        .add_idl_str("main.smithy", main.replace("    g: Nowhere\n", ""))
        .add_idl_str("other.smithy", other)
        .add_json_str("extra.json", json)
        .add_shape(
            SimpleShape::builder(ShapeType::Long, id("ex#FromCode"))
                .build()
                .unwrap(),
        )
        .add_model(&existing)
        .assemble()
        .into_result()
        .unwrap();
    assert_eq!(target(&model, "ex#S$a"), "other#String");
    assert_eq!(target(&model, "ex#S$b"), "ex#Later");
    assert_eq!(target(&model, "ex#S$c"), "ex#FromJson");
    assert_eq!(target(&model, "ex#S$d"), "ex#FromCode");
    assert_eq!(target(&model, "ex#S$e"), "smithy.api#Integer");
    assert_eq!(target(&model, "ex#S$f"), "ex#Boolean");
}

#[test]
fn later_idl_file_shadows_the_prelude() {
    let model = Model::assembler()
        .add_idl_str("a.smithy", "namespace ex\nstructure S { a: String }\n")
        .add_idl_str("b.smithy", "namespace ex\ninteger String\n")
        .assemble()
        .into_result()
        .unwrap();
    assert_eq!(target(&model, "ex#S$a"), "ex#String");
}

#[test]
fn other_namespaces_do_not_shadow() {
    let model = Model::assembler()
        .add_idl_str("a.smithy", "namespace ex\nstructure S { a: String }\n")
        .add_idl_str("b.smithy", "namespace other\ninteger String\n")
        .assemble()
        .into_result()
        .unwrap();
    assert_eq!(target(&model, "ex#S$a"), "smithy.api#String");
}

#[test]
fn json_references_idl_shapes_and_idl_references_json_traits() {
    let model = Model::assembler()
        .add_json_str(
            "a.json",
            r#"{"smithy": "2.0", "shapes": {
                "ex#FromJson": {"type": "structure", "members": {"a": {"target": "ex#FromIdl"}}},
                "ex#jsonTrait": {"type": "structure", "members": {},
                    "traits": {"smithy.api#trait": {}}}
            }}"#,
        )
        .add_idl_str("b.smithy", "namespace ex\n@jsonTrait\nstring FromIdl\n")
        .assemble()
        .into_result()
        .unwrap();
    let shape = model.expect_shape(&id("ex#FromIdl")).unwrap();
    assert!(shape.traits().contains(&id("ex#jsonTrait")));
    assert_eq!(target(&model, "ex#FromJson$a"), "ex#FromIdl");
}

#[test]
fn member_ids_resolve_their_root() {
    let model = idl("namespace ex
@trait
structure ref { target: String }

@ref(target: S$a)
structure S { a: String }
");
    let shape = model.expect_shape(&id("ex#S")).unwrap();
    let value = shape.traits().get(&id("ex#ref")).unwrap();
    assert_eq!(value.to_json_string(), r#"{"target":"ex#S$a"}"#);
}

#[test]
fn quoted_entity_references_resolve_like_unquoted_ones() {
    // Entity properties are references, so a quoted relative ID still resolves and is never a
    // syntactic shape ID warning.
    let validated = Model::assembler()
        .add_idl_str(
            "model.smithy",
            "namespace ex\nservice S { operations: [\"Op\"] }\noperation Op {}\n",
        )
        .assemble();
    assert!(
        validated.diagnostics().is_empty(),
        "{:?}",
        validated.diagnostics()
    );
    let model = validated.into_result().unwrap();
    assert!(json(&model).contains(r#""operations":[{"target":"ex#Op"}]"#));
}

// --- Sugar -------------------------------------------------------------------------------

#[test]
fn every_entity_property_lowers_like_its_json_ast() {
    let other = r#"{"smithy": "2.0", "shapes": {"other#Renamed": {"type": "string"}}}"#;
    let idl_model = Model::assembler()
        .add_json_str("other.json", other)
        .add_idl_str(
            "model.smithy",
            "namespace ex
use other#Renamed

service Service {
    version: \"1\"
    operations: [Op]
    resources: [R]
    errors: [E]
    rename: { \"other#Renamed\": \"Local\" }
}
resource R {
    identifiers: { id: String }
    properties: { p: String }
    create: Op
    put: Put
    read: Read
    update: Op2
    delete: Delete
    list: List
    operations: [Op3]
    collectionOperations: [Op4]
    resources: [Child]
}
resource Child {}
operation Op { input: Input, output: Output, errors: [E] }
@idempotent operation Put {}
@readonly operation Read {}
operation Op2 {}
@idempotent operation Delete {}
@readonly operation List {}
operation Op3 {}
operation Op4 {}
structure Input { r: Renamed }
structure Output {}
@error(\"client\") structure E {}
",
        )
        .assemble()
        .into_result();
    let json_model = Model::assembler()
        .add_json_str("other.json", other)
        .add_json_str(
            "model.json",
            r#"{"smithy": "2.0", "shapes": {
            "ex#Service": {"type": "service", "version": "1",
                "operations": [{"target": "ex#Op"}], "resources": [{"target": "ex#R"}],
                "errors": [{"target": "ex#E"}], "rename": {"other#Renamed": "Local"}},
            "ex#R": {"type": "resource",
                "identifiers": {"id": {"target": "smithy.api#String"}},
                "properties": {"p": {"target": "smithy.api#String"}},
                "create": {"target": "ex#Op"}, "put": {"target": "ex#Put"},
                "read": {"target": "ex#Read"}, "update": {"target": "ex#Op2"},
                "delete": {"target": "ex#Delete"}, "list": {"target": "ex#List"},
                "operations": [{"target": "ex#Op3"}],
                "collectionOperations": [{"target": "ex#Op4"}],
                "resources": [{"target": "ex#Child"}]},
            "ex#Child": {"type": "resource"},
            "ex#Op": {"type": "operation", "input": {"target": "ex#Input"},
                "output": {"target": "ex#Output"}, "errors": [{"target": "ex#E"}]},
            "ex#Put": {"type": "operation", "traits": {"smithy.api#idempotent": {}}},
            "ex#Read": {"type": "operation", "traits": {"smithy.api#readonly": {}}},
            "ex#Op2": {"type": "operation"},
            "ex#Delete": {"type": "operation", "traits": {"smithy.api#idempotent": {}}},
            "ex#List": {"type": "operation", "traits": {"smithy.api#readonly": {}}},
            "ex#Op3": {"type": "operation"},
            "ex#Op4": {"type": "operation"},
            "ex#Input": {"type": "structure", "members": {"r": {"target": "other#Renamed"}}},
            "ex#Output": {"type": "structure", "members": {}},
            "ex#E": {"type": "structure", "members": {}, "traits": {"smithy.api#error": "client"}}
        }}"#,
        )
        .assemble()
        .into_result()
        .unwrap();
    let idl_model = idl_model.unwrap();
    assert!(
        idl_model.equivalent(&json_model),
        "\n{}\n{}",
        json(&idl_model),
        json(&json_model)
    );
    assert_eq!(json(&idl_model), json(&json_model));
}

#[test]
fn unknown_entity_properties_are_reported_like_json() {
    let err = idl_err("namespace ex\nservice S { version: \"1\", bogus: true }\n");
    assert_eq!(codes(&err), [DiagnosticCode::UnknownProperty]);
    let location = err
        .diagnostics()
        .iter()
        .next()
        .unwrap()
        .source_location()
        .unwrap();
    assert_eq!((location.line(), location.column()), (Some(2), Some(27)));

    let err = idl_err("namespace ex\nresource R { read: [Op] }\noperation Op {}\n");
    assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);
}

#[test]
fn inline_io_order_traits_and_collisions() {
    let model = idl("namespace ex
operation B {
    output := @documentation(\"out\") { o: String }
    input := { i: String }
}
operation A {}
");
    let order: Vec<_> = model
        .non_prelude_shapes()
        .map(|s| s.id().to_string())
        .collect();
    assert_eq!(order, ["ex#BInput", "ex#BOutput", "ex#B", "ex#A"]);
    let output = model.expect_shape(&id("ex#BOutput")).unwrap();
    assert!(output.traits().contains(&id("smithy.api#output")));
    assert!(output.traits().contains(&id("smithy.api#documentation")));

    // A declared shape with the generated name collides, at the declaration.
    let err = idl_err("namespace ex\noperation A { input := {} }\nstructure AInput {}\n");
    assert_eq!(
        codes(&err),
        [DiagnosticCode::ShapeConflict],
        "{:?}",
        err.diagnostics()
    );

    // So does a generated shape from another file.
    let err = Model::assembler()
        .add_idl_str("a.smithy", "namespace ex\noperation A { input := {} }\n")
        .add_idl_str("b.smithy", "namespace ex\nstring AInput\n")
        .assemble()
        .into_result()
        .unwrap_err();
    assert_eq!(codes(&err), [DiagnosticCode::ShapeConflict]);

    // An inline structure carrying its own `@input` merges with the generated one.
    let model = idl("namespace ex\noperation A { input := @input {} }\n");
    assert!(model.expect_shape(&id("ex#AInput")).is_ok());

    // Errors cannot be inline.
    let err = idl_err("namespace ex\noperation A { errors := {} }\n");
    assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);
}

#[test]
fn target_elision_in_every_placement() {
    let model = Model::assembler()
        .add_idl_str(
            "a.smithy",
            "namespace ex
use other#Remote

resource Local { identifiers: { id: Id } }
string Id

structure InStructure for Local { $id }
union InUnion for Local { $id }
operation Op { input := for Local { $id }, output := for Remote { $remoteId } }
",
        )
        .add_json_str(
            "b.json",
            r#"{"smithy": "2.0", "shapes": {
                "other#Remote": {"type": "resource",
                    "identifiers": {"remoteId": {"target": "other#RemoteId"}}},
                "other#RemoteId": {"type": "string"}
            }}"#,
        )
        .assemble()
        .into_result()
        .unwrap();
    assert_eq!(target(&model, "ex#InStructure$id"), "ex#Id");
    assert_eq!(target(&model, "ex#InUnion$id"), "ex#Id");
    assert_eq!(target(&model, "ex#OpInput$id"), "ex#Id");
    assert_eq!(target(&model, "ex#OpOutput$remoteId"), "other#RemoteId");

    // Elision without `for` is an error at the `$`.
    let err = idl_err("namespace ex\nstructure S {\n    $id\n}\n");
    assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);
    let location = err
        .diagnostics()
        .iter()
        .next()
        .unwrap()
        .source_location()
        .unwrap();
    assert_eq!((location.line(), location.column()), (Some(3), Some(5)));

    // Identifier matching is case-sensitive.
    let err = idl_err(
        "namespace ex\nresource R { identifiers: { id: String } }\nstructure S for R { $ID }\n",
    );
    assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);

    // Mixin-based elision is reported once, as the unsupported mixin.
    let err =
        idl_err("namespace ex\n@mixin structure M { id: String }\nstructure S with [M] { $id }\n");
    assert!(
        !codes(&err).is_empty()
            && codes(&err)
                .iter()
                .all(|c| *c == DiagnosticCode::UnsupportedFeature),
        "{:?}",
        err.diagnostics()
    );
}

#[test]
fn documentation_and_assignment_sugar() {
    let model = idl("namespace ex
/// Line one
///
///   indented
enum E {
    /// Member
    A = \"a\"
    B
}
intEnum I {
    X = 1
}
structure S {
    n: Integer = 1
    @documentation(\"explicit\")
    d: String = \"\"
}
");
    let e = model.expect_shape(&id("ex#E")).unwrap();
    assert_eq!(
        e.traits()
            .get(&id("smithy.api#documentation"))
            .unwrap()
            .to_json_string(),
        r#""Line one\n\n  indented""#
    );
    let json = json(&model);
    assert!(json.contains(r#""A":{"target":"smithy.api#Unit","traits":{"smithy.api#documentation":"Member","smithy.api#enumValue":"a"}}"#), "{json}");
    assert!(
        json.contains(r#""B":{"target":"smithy.api#Unit"}"#),
        "{json}"
    );
    assert!(
        json.contains(r#""X":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":1}}"#),
        "{json}"
    );
    assert!(
        json.contains(r#""n":{"target":"smithy.api#Integer","traits":{"smithy.api#default":1}}"#),
        "{json}"
    );
    assert!(json.contains(r#""smithy.api#default":"""#), "{json}");

    // Assignment sugar conflicts with an explicit, different trait.
    let err = idl_err("namespace ex\nstructure S {\n    @default(2)\n    n: Integer = 1\n}\n");
    assert_eq!(codes(&err), [DiagnosticCode::TraitConflict]);
}

// --- Locations ---------------------------------------------------------------------------

/// The diagnostic position for a JSON pointer in both representations of the same model.
#[test]
fn equivalent_inputs_have_equal_pointers() {
    let idl_err = idl_err(
        "namespace ex
structure S {
    @length(min: \"x\")
    a: Missing
}
",
    );
    let json_err = Model::from_json_str(
        "model.json",
        r#"{"smithy": "2.0", "shapes": {"ex#S": {"type": "structure", "members": {
            "a": {"target": "ex#Missing", "traits": {"smithy.api#length": {"min": "x"}}}}}}}"#,
    )
    .unwrap_err();
    let pointers = |err: &LoadError| {
        let mut values: Vec<_> = err
            .diagnostics()
            .iter()
            .map(|d| {
                (
                    d.code(),
                    d.source_location()
                        .and_then(|l| l.pointer())
                        .map(str::to_owned),
                )
            })
            .collect();
        values.sort_by_key(|(code, pointer)| (format!("{code:?}"), pointer.clone()));
        values
    };
    assert_eq!(pointers(&idl_err), pointers(&json_err));
    let missing = idl_err
        .diagnostics()
        .iter()
        .find(|d| d.code() == DiagnosticCode::UnresolvedReference)
        .unwrap()
        .source_location()
        .unwrap();
    assert_eq!((missing.line(), missing.column()), (Some(4), Some(8)));
}

#[test]
fn every_semantic_origin_has_its_documented_position() {
    // Collected through `SourceLocation::child` on the shape locations, which is how trait
    // decoders and validators locate nested values.
    let model = idl("namespace ex
/// Docs
@tags([\"t\", \"ü\"])
@length(min: 1)
string S

enum E {
    A
    B = \"b\"
}

operation Op {
    input := {
        a: String = \"x\"
    }
}
");
    let pos = |location: &aws_smithy_lang::SourceLocation| {
        (location.line().unwrap(), location.column().unwrap())
    };
    let shape = |name: &str| model.expect_shape(&id(name)).unwrap();
    let trait_value =
        |name: &str, trait_id: &str| shape(name).traits().get(&id(trait_id)).unwrap().clone();
    let member = |root: &str, name: &str| shape(root).members().get(name).unwrap();

    // Shape: its name. Documentation: the first `///`. Traits: the `@`.
    assert_eq!(pos(shape("ex#S").source_location()), (5, 8));
    assert_eq!(
        pos(trait_value("ex#S", "smithy.api#documentation").source_location()),
        (2, 1)
    );
    let tags = trait_value("ex#S", "smithy.api#tags");
    assert_eq!(pos(tags.source_location()), (3, 1));
    // A string in an array: the string itself, in characters.
    assert_eq!(pos(tags.as_array().unwrap()[1].source_location()), (3, 13));
    // An object value: its key.
    let length = trait_value("ex#S", "smithy.api#length");
    assert_eq!(
        pos(length
            .as_object()
            .unwrap()
            .get("min")
            .unwrap()
            .source_location()),
        (4, 9)
    );
    assert_eq!(
        length
            .as_object()
            .unwrap()
            .get("min")
            .unwrap()
            .source_location()
            .pointer(),
        Some("/shapes/ex#S/traits/smithy.api#length/min")
    );
    // Enum members: the member name; an `= value` assignment: the value.
    assert_eq!(pos(member("ex#E", "A").source_location()), (8, 5));
    let b = member("ex#E", "B");
    assert_eq!(
        pos(b
            .traits()
            .get(&id("smithy.api#enumValue"))
            .unwrap()
            .source_location()),
        (9, 9)
    );
    // Inline shapes and their generated traits: the `:=`.
    assert_eq!(pos(shape("ex#OpInput").source_location()), (13, 11));
    assert_eq!(
        pos(trait_value("ex#OpInput", "smithy.api#input").source_location()),
        (13, 11)
    );
    let a = member("ex#OpInput", "a");
    assert_eq!(
        pos(a
            .traits()
            .get(&id("smithy.api#default"))
            .unwrap()
            .source_location()),
        (14, 21)
    );
}

#[test]
fn conflicts_between_idl_and_json_name_both_sources() {
    let err = Model::assembler()
        .add_idl_str("a.smithy", "namespace ex\n\nstring S\n")
        .add_json_str(
            "b.json",
            r#"{"smithy": "2.0", "shapes": {"ex#S": {"type": "integer"}}}"#,
        )
        .assemble()
        .into_result()
        .unwrap_err();
    assert_eq!(codes(&err), [DiagnosticCode::ShapeConflict]);
    let diagnostic = err.diagnostics().iter().next().unwrap();
    let rendered = format!("{diagnostic}");
    let related: Vec<_> = diagnostic.related().iter().map(|r| r.to_string()).collect();
    let all = format!("{rendered} {related:?}");
    assert!(all.contains("a.smithy:3:8"), "{all}");
    assert!(all.contains("b.json:1"), "{all}");

    // Equal definitions in both formats merge silently.
    Model::assembler()
        .add_idl_str("a.smithy", "namespace ex\n@sensitive\nstring S\n")
        .add_json_str(
            "b.json",
            r#"{"smithy": "2.0", "shapes": {"ex#S": {"type": "string",
                "traits": {"smithy.api#sensitive": {}}}}}"#,
        )
        .assemble()
        .into_result()
        .unwrap();
}

// --- Limits and diagnostics --------------------------------------------------------------

#[test]
fn limits_bound_idl_input() {
    let shapes: String = (0..20).map(|i| format!("string S{i}\n")).collect();
    let err = Model::assembler()
        .max_shapes(10)
        .add_idl_str("model.smithy", format!("namespace ex\n{shapes}"))
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_resource_limit());

    let err = Model::assembler()
        .max_input_bytes(16)
        .add_idl_str("model.smithy", "namespace ex\nstring LongEnoughName\n")
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_resource_limit());

    let err = Model::assembler()
        .max_depth(3)
        .add_idl_str("model.smithy", "metadata m = [[[[1]]]]\n")
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_resource_limit());
    Model::assembler()
        .max_depth(4)
        .add_idl_str("model.smithy", "metadata m = [[[[1]]]]\n")
        .assemble()
        .into_result()
        .unwrap();

    // Errors across IDL inputs stop at max_diagnostics.
    let mut assembler = Model::assembler().max_diagnostics(5);
    for i in 0..10 {
        assembler = assembler.add_idl_str(
            format!("{i}.smithy"),
            "namespace ex\nstructure S { a: Missing }\n".replace('S', &format!("S{i}")),
        );
    }
    let err = assembler.assemble().into_result().unwrap_err();
    assert_eq!(err.diagnostics().len(), 6);
    assert!(
        err.is_resource_limit()
            || err
                .diagnostics()
                .iter()
                .any(|d| d.code() == DiagnosticCode::ResourceLimit)
    );
}

#[test]
fn independent_inputs_report_independently() {
    // A syntax error in one input does not hide another input's errors.
    let err = Model::assembler()
        .add_idl_str("a.smithy", "namespace ex\nstructure {\n")
        .add_idl_str("b.smithy", "namespace ex\nstructure S { a: Missing }\n")
        .add_json_str("c.json", "{")
        .assemble()
        .into_result()
        .unwrap_err();
    let mut seen = codes(&err);
    seen.sort_by_key(|code| format!("{code:?}"));
    assert_eq!(
        seen,
        [DiagnosticCode::IdlSyntax, DiagnosticCode::JsonSyntax],
        "unresolved references are only checked once every input loads: {:?}",
        err.diagnostics()
    );
    assert!(err.is_idl_syntax());
}

#[test]
fn warnings_do_not_fail_assembly() {
    let validated = Model::assembler()
        .add_idl_str(
            "model.smithy",
            "$a: 1\n$b: 2\nmetadata m = [Missing]\nnamespace ex\n@tags([X])\nstring S\n",
        )
        .assemble();
    let severities: Vec<_> = validated
        .diagnostics()
        .iter()
        .map(|d| (d.code(), d.severity()))
        .collect();
    assert_eq!(severities.len(), 4, "{severities:?}");
    assert!(severities.iter().all(|(_, s)| *s == Severity::Warning));
    assert!(validated.into_result().is_ok());
}

// --- Robustness --------------------------------------------------------------------------

const SAMPLE: &str = include_str!("fixtures/idl/sugar.smithy");

fn check(input: &[u8]) -> Result<Model, LoadError> {
    Model::assembler()
        .max_depth(16)
        .max_shapes(64)
        .max_diagnostics(16)
        .add_idl_slice("fuzz.smithy", input.to_vec())
        .add_json_str(
            "other.json",
            r#"{"smithy": "2.0", "shapes": {"other#Imported": {"type": "blob"}}}"#,
        )
        .assemble()
        .into_result()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_panic(input in prop::collection::vec(any::<u8>(), 0..256)) {
        let _ = check(&input);
    }

    #[test]
    fn truncated_documents_never_panic(len in 0..SAMPLE.len()) {
        let _ = check(&SAMPLE.as_bytes()[..len]);
    }

    #[test]
    fn mutated_documents_never_panic(
        edits in prop::collection::vec((0..SAMPLE.len(), any::<u8>()), 1..4),
    ) {
        let mut input = SAMPLE.as_bytes().to_vec();
        for (index, byte) in edits {
            input[index] = byte;
        }
        let _ = check(&input);
    }

    #[test]
    fn token_insertions_never_panic(
        index in 0..SAMPLE.len(),
        insert in prop::sample::select(vec![
            "{", "}", "[", "]", "(", ")", ",", ":", ":=", "=", "\"", "\"\"\"", "\"\"\"\n", "\n",
            "///", "//", "@", "$", "#", "apply ", "with [", "for ", "use ", "namespace ",
            "metadata ", "1e999", "-", "\\", "\r", "é", "\u{0}",
        ]),
    ) {
        let mut input = SAMPLE.to_owned();
        let index = (0..=index).rev().find(|i| input.is_char_boundary(*i)).unwrap();
        input.insert_str(index, insert);
        let _ = check(input.as_bytes());
    }

    /// Every accepted model round-trips through the JSON AST.
    #[test]
    fn accepted_mutations_round_trip(
        edits in prop::collection::vec((0..SAMPLE.len(), any::<u8>()), 1..3),
    ) {
        let mut input = SAMPLE.as_bytes().to_vec();
        for (index, byte) in edits {
            input[index] = byte;
        }
        if let Ok(model) = check(&input) {
            let written = AstWriter::new().to_string(&model).unwrap();
            let reloaded = Model::assembler()
                .add_json_str("written.json", written)
                .assemble()
                .into_result()
                .unwrap();
            prop_assert!(model.equivalent(&reloaded));
        }
    }
}

#[test]
fn deep_and_long_inputs_are_bounded() {
    // Nesting beyond the depth limit is a resource limit, never a stack overflow.
    let deep = format!(
        "metadata m = {}{}",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    assert!(idl_err(&deep).is_resource_limit());
    let deep_trait = format!(
        "namespace ex\n@tags({}{})\nstring S\n",
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    assert!(idl_err(&deep_trait).is_resource_limit());
    // Long comment, string, and text-block runs scan linearly.
    let long = format!(
        "// {}\nmetadata a = \"{}\"\nmetadata b = \"\"\"\n{}\n\"\"\"\n",
        "x".repeat(1 << 20),
        "y".repeat(1 << 20),
        "    z\n".repeat(1 << 16)
    );
    idl(&long);
}

// --- Repository corpus -------------------------------------------------------------------

/// Every `.smithy` file checked into the repository, outside build output and this crate's own
/// fixtures (which include deliberately invalid models).
fn repository_models() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if path.is_dir() {
                if path.ends_with("aws-smithy-lang/tests/fixtures") {
                    continue;
                }
                if !matches!(
                    name.as_ref(),
                    "build" | "target" | ".git" | "node_modules" | ".gradle"
                ) {
                    stack.push(path);
                }
            } else if name.ends_with(".smithy") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// How a repository model is expected to load on its own.
#[derive(Debug, PartialEq)]
enum Expected {
    /// Parses and lowers; any errors are semantic (for example, unresolved references to
    /// traits defined in other packages).
    Parses,
    /// Rejected because it uses a construct this crate deliberately does not support.
    Unsupported,
}

fn classify(path: &Path) -> Expected {
    let text = std::fs::read_to_string(path).unwrap();
    let uses_unsupported = text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("$version: \"1")
            || line.starts_with("apply ")
            || line.starts_with("@mixin")
            || line.contains(" with [")
            || line.starts_with("set ")
    });
    if uses_unsupported {
        Expected::Unsupported
    } else {
        Expected::Parses
    }
}

#[test]
fn repository_models_are_classified() {
    let models = repository_models();
    assert!(models.len() >= 20, "found only {} models", models.len());
    let mut parsed = 0;
    let mut failures: Vec<String> = Vec::new();
    for path in &models {
        let result = Model::from_idl_file(path);
        let shown = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(path)
            .display();
        match (classify(path), result) {
            (Expected::Parses, Ok(_)) => parsed += 1,
            (Expected::Parses, Err(err)) => {
                // Files are loaded alone, so references to shapes and traits in other files
                // may not resolve. Nothing else may fail.
                for d in err.diagnostics() {
                    if !matches!(
                        d.code(),
                        DiagnosticCode::UnresolvedReference
                            | DiagnosticCode::InvalidTarget
                            | DiagnosticCode::InvalidTrait
                    ) {
                        failures.push(format!("{shown}: unexpected {d}"));
                    }
                }
            }
            (Expected::Unsupported, Ok(_)) => {
                failures.push(format!("{shown}: uses an unsupported construct"));
            }
            (Expected::Unsupported, Err(err)) => {
                let precise = !err.is_idl_syntax()
                    && err.diagnostics().iter().any(|d| {
                        matches!(
                            d.code(),
                            DiagnosticCode::UnsupportedFeature | DiagnosticCode::UnsupportedVersion
                        )
                    });
                if !precise {
                    let all: Vec<_> = err.diagnostics().iter().map(|d| d.to_string()).collect();
                    failures.push(format!("{shown}: imprecise rejection: {all:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "{parsed} of {} repository models load on their own",
        models.len()
    );
    assert!(
        parsed >= 5,
        "only {parsed} repository models load on their own"
    );
}
