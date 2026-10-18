/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy IDL loading: equivalence with the JSON AST, resolution across inputs, sugar, and
//! diagnostics.

use aws_smithy_lang::diagnostic::{DiagnosticCode, Severity};
use aws_smithy_lang::{AstWriter, LoadError, Model, ShapeId};

const OTHER: &str = r#"{"smithy": "2.0", "shapes": {"other#Imported": {"type": "blob"}}}"#;

fn render(err: &LoadError) -> String {
    let diagnostics: Vec<_> = err.diagnostics().iter().map(|d| d.to_string()).collect();
    format!("{err}\n{}", diagnostics.join("\n"))
}

fn idl(input: &str) -> Model {
    Model::from_idl_str("model.smithy", input).unwrap_or_else(|err| panic!("{}", render(&err)))
}

fn idl_err(input: &str) -> LoadError {
    Model::from_idl_str("model.smithy", input).unwrap_err()
}

fn codes(err: &LoadError) -> Vec<DiagnosticCode> {
    err.diagnostics().iter().map(|d| d.code()).collect()
}

/// Asserts semantic equality and identical canonical JSON.
fn assert_same(idl_model: &Model, json_model: &Model) {
    let writer = AstWriter::new();
    assert_eq!(
        writer.to_string(idl_model).unwrap(),
        writer.to_string(json_model).unwrap()
    );
    assert!(idl_model.equivalent(json_model));
}

fn id(value: &str) -> ShapeId {
    ShapeId::new(value).unwrap()
}

#[test]
fn every_construct_matches_its_json_ast() {
    let from_idl = Model::assembler()
        .add_json_str("other.json", OTHER)
        .add_idl_str("sugar.smithy", include_str!("fixtures/idl/sugar.smithy"))
        .assemble()
        .into_result()
        .unwrap_or_else(|err| panic!("{}", render(&err)));
    let from_json = Model::assembler()
        .add_json_str("other.json", OTHER)
        .add_json_str("sugar.json", include_str!("fixtures/idl/sugar.json"))
        .assemble()
        .into_result()
        .unwrap_or_else(|err| panic!("{}", render(&err)));
    assert_same(&from_idl, &from_json);

    // Declaration order, with generated inline input and output before their operation.
    let order: Vec<_> = from_idl
        .non_prelude_shapes()
        .map(|s| s.id().as_str().to_owned())
        .filter(|s| s.starts_with("smithy.example#GetThing"))
        .collect();
    assert_eq!(
        order,
        [
            "smithy.example#GetThingInput",
            "smithy.example#GetThingOutput",
            "smithy.example#GetThing",
        ]
    );
}

#[test]
fn relative_ids_resolve_across_inputs_and_formats() {
    // `Name` is declared in a later JSON input and shadows nothing; `String` is declared in
    // the namespace by a later IDL input and so shadows the prelude.
    let model = Model::assembler()
        .add_idl_str(
            "a.smithy",
            "$version: \"2\"\nnamespace ex\nstructure Person { name: Name, nick: String }\n",
        )
        .add_json_str(
            "b.json",
            r#"{"smithy": "2.0", "shapes": {"ex#Name": {"type": "string"},
                "ex#Ref": {"type": "structure", "members": {"p": {"target": "ex#Person"}}}}}"#,
        )
        .add_idl_str("c.smithy", "namespace ex\ninteger String\n")
        .assemble()
        .into_result()
        .unwrap_or_else(|err| panic!("{}", render(&err)));
    let person = model.expect_shape(&id("ex#Person")).unwrap();
    let targets: Vec<_> = person
        .members()
        .iter()
        .map(|m| m.target().as_str().to_owned())
        .collect();
    assert_eq!(targets, ["ex#Name", "ex#String"]);
}

#[test]
fn prelude_resolution_ignores_disable_prelude() {
    let err = Model::assembler()
        .disable_prelude()
        .add_idl_str("a.smithy", "namespace ex\nstructure S { a: String }\n")
        .assemble()
        .into_result()
        .unwrap_err();
    let unresolved = err
        .diagnostics()
        .iter()
        .find(|d| d.code() == DiagnosticCode::UnresolvedReference)
        .expect("an unresolved prelude reference");
    assert!(
        unresolved.message().contains("smithy.api#String"),
        "{unresolved}"
    );
}

#[test]
fn omitted_trait_values_follow_the_trait_shape() {
    let model = idl(r#"
namespace ex

@trait
list listTrait { member: String }

@trait
string stringTrait

@listTrait
@stringTrait
@unknownTrait
@required
structure S {}
"#);
    let shape = model.expect_shape(&id("ex#S")).unwrap();
    let value = |trait_id: &str| shape.traits().get(&id(trait_id)).unwrap().to_json_string();
    assert_eq!(value("ex#listTrait"), "[]");
    assert_eq!(value("ex#stringTrait"), "null");
    assert_eq!(value("ex#unknownTrait"), "{}");
    assert_eq!(value("smithy.api#required"), "{}");
}

#[test]
fn repeated_traits_follow_merge_rules() {
    let model =
        idl("namespace ex\n@required @required()\n@tags([\"a\"]) @tags([\"b\"])\nstructure S {}\n");
    let shape = model.expect_shape(&id("ex#S")).unwrap();
    assert_eq!(
        shape
            .traits()
            .get(&id("smithy.api#tags"))
            .unwrap()
            .to_json_string(),
        r#"["a","b"]"#
    );
    let err = idl_err("namespace ex\n@length(min: 1)\n@length(min: 2)\nstring S\n");
    assert_eq!(codes(&err), [DiagnosticCode::TraitConflict]);
    let location = err
        .diagnostics()
        .iter()
        .next()
        .unwrap()
        .source_location()
        .unwrap();
    assert_eq!(location.line(), Some(3));
}

#[test]
fn resource_target_elision() {
    let model = idl(r#"
namespace ex

resource User {
    identifiers: { name: UserName }
}

string UserName

structure Summary for User {
    $name
    age: Integer
}

operation GetUser {
    input := for User {
        @required
        $name
    }
}
"#);
    for shape in ["ex#Summary", "ex#GetUserInput"] {
        let shape = model.expect_shape(&id(shape)).unwrap();
        assert_eq!(
            shape.members().iter().next().unwrap().target(),
            &id("ex#UserName")
        );
    }

    let err = idl_err("namespace ex\nresource R {}\nstructure S for R { $missing }\n");
    assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);
    let err = idl_err("namespace ex\nstructure S { $missing }\n");
    assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);
    let err = idl_err("namespace ex\nstring R\nstructure S for R { a: String }\n");
    assert_eq!(codes(&err), [DiagnosticCode::InvalidTarget]);

    // `ForResource` is valid on every aggregate shape.
    let model = idl(
        "namespace ex\nresource R { identifiers: { id: String } }\nunion U for R { $id }\nlist L for R { member: String }\n",
    );
    let union = model.expect_shape(&id("ex#U")).unwrap();
    assert_eq!(
        union.members().iter().next().unwrap().target(),
        &id("smithy.api#String")
    );
    assert!(idl_err("namespace ex\nresource R {}\nstring S for R\n").is_idl_syntax());
}

#[test]
fn commas_do_not_end_statements() {
    for input in [
        "$version: \"2\",\n",
        "metadata m = 1,\n",
        "namespace ex,\n",
        "namespace ex\nuse other#T,\n",
        "namespace ex\nstring S,\n",
    ] {
        assert!(idl_err(input).is_idl_syntax(), "accepted {input:?}");
    }
    // Commas remain whitespace after the line break.
    idl("namespace ex\n,string S\n");
}

#[test]
fn inline_io_suffix_controls() {
    let model = idl(r#"
$version: "2.0"
$operationInputSuffix: "Request"
$operationOutputSuffix: "Response"
namespace ex

operation Ping {
    input := {}
    output := @sensitive {}
}
"#);
    let input = model.expect_shape(&id("ex#PingRequest")).unwrap();
    assert!(input.traits().contains(&id("smithy.api#input")));
    let output = model.expect_shape(&id("ex#PingResponse")).unwrap();
    assert!(output.traits().contains(&id("smithy.api#output")));
    assert!(output.traits().contains(&id("smithy.api#sensitive")));

    let err = idl_err("namespace ex\nstructure PingInput {}\noperation Ping { input := {} }\n");
    assert_eq!(codes(&err), [DiagnosticCode::ShapeConflict]);
}

#[test]
fn syntactic_shape_ids_resolve_and_warn() {
    let validated = Model::assembler()
        .add_idl_str(
            "a.smithy",
            "metadata m = [required, Missing]\nnamespace ex\n@tags([Thing, Nope])\nstring Thing\n",
        )
        .assemble();
    let warnings: Vec<_> = validated
        .diagnostics()
        .iter()
        .filter(|d| d.code() == DiagnosticCode::SyntacticShapeIdTarget)
        .map(|d| (d.severity(), d.message().to_owned()))
        .collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().all(|(s, _)| *s == Severity::Warning));
    assert!(warnings[0].1.contains("smithy.api#Missing"));
    assert!(warnings[1].1.contains("ex#Nope"));
    let model = validated.into_result().unwrap();
    assert_eq!(
        model.metadata_value("m").unwrap().to_json_string(),
        r#"["smithy.api#required","smithy.api#Missing"]"#
    );
    let thing = model.expect_shape(&id("ex#Thing")).unwrap();
    assert_eq!(
        thing
            .traits()
            .get(&id("smithy.api#tags"))
            .unwrap()
            .to_json_string(),
        r#"["ex#Thing","ex#Nope"]"#
    );
}

#[test]
fn unsupported_constructs_are_reported_precisely() {
    let err = idl_err(
        "namespace ex\nstring S\napply S @sensitive\nstructure M with [S] {}\nset Old { member: String }\n",
    );
    assert_eq!(
        codes(&err),
        [
            DiagnosticCode::UnsupportedFeature,
            DiagnosticCode::UnsupportedFeature,
            DiagnosticCode::UnsupportedFeature,
        ]
    );
    assert!(err.is_invalid_model());
    let err = idl_err("namespace ex\n@mixin\nstructure M {}\n");
    assert_eq!(codes(&err), [DiagnosticCode::UnsupportedFeature]);
}

#[test]
fn versions_and_controls() {
    assert!(Model::from_idl_str("a.smithy", "$version: \"2\"\n").is_ok());
    let err = idl_err("$version: \"1.0\"\nnamespace ex\nstring S\nstring T\n@bad(\n");
    // A syntax error is reported before the version is examined.
    assert!(err.is_idl_syntax());
    let err = idl_err("$version: \"1.0\"\nnamespace ex\nstructure S { a: Missing }\n");
    assert_eq!(codes(&err), [DiagnosticCode::UnsupportedVersion]);
    let err = idl_err("$version: \"two\"\n");
    assert_eq!(codes(&err), [DiagnosticCode::InvalidProperty]);
    let validated = Model::assembler()
        .add_idl_str("a.smithy", "$custom: true\n")
        .assemble();
    assert_eq!(validated.diagnostics().len(), 1);
    assert_eq!(
        validated.diagnostics().iter().next().unwrap().severity(),
        Severity::Warning
    );
    assert!(validated.into_result().is_ok());
}

#[test]
fn syntax_errors_have_exact_locations() {
    // Columns count characters: `é` is two bytes but one column.
    let err = idl_err(
        "namespace ex\nstructure S {\n    a: String\n    @documentation(\"é\") b String\n}\n",
    );
    assert!(err.is_idl_syntax());
    assert_eq!(codes(&err), [DiagnosticCode::IdlSyntax]);
    let location = err
        .diagnostics()
        .iter()
        .next()
        .unwrap()
        .source_location()
        .unwrap();
    assert_eq!((location.line(), location.column()), (Some(4), Some(27)));
    assert!(
        err.to_string()
            .starts_with("failed to parse Smithy IDL at model.smithy:4:27"),
        "{err}"
    );

    let err = Model::from_idl_slice("bad.smithy", b"namespace ex\nstring \xff").unwrap_err();
    assert!(err.is_idl_syntax());
    let location = err
        .diagnostics()
        .iter()
        .next()
        .unwrap()
        .source_location()
        .unwrap();
    assert_eq!((location.line(), location.column()), (Some(2), Some(8)));

    let err = idl_err("namespace ex\nstructure S { a: String, a: Integer }\n");
    assert!(err.is_idl_syntax());
    assert_eq!(codes(&err), [DiagnosticCode::DuplicateKey]);
}

#[test]
fn limits_apply_to_idl() {
    let input = "namespace ex\nstring A\nstring B\n";
    let err = Model::assembler()
        .max_shapes(1)
        .add_idl_str("a.smithy", input)
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_resource_limit());
    let err = Model::assembler()
        .max_input_bytes(4)
        .add_idl_str("a.smithy", input)
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_resource_limit());
    let err = Model::assembler()
        .max_depth(2)
        .add_idl_str("a.smithy", "metadata m = [[[1]]]\n")
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_resource_limit());
}

#[test]
fn semantic_locations_point_into_idl() {
    let input = "namespace ex\n\nstructure S {\n    a: Missing\n}\n";
    let err = idl_err(input);
    let diagnostic = err
        .diagnostics()
        .iter()
        .find(|d| d.code() == DiagnosticCode::UnresolvedReference)
        .unwrap();
    let location = diagnostic.source_location().unwrap();
    assert_eq!(location.pointer(), Some("/shapes/ex#S/members/a/target"));
    assert_eq!((location.line(), location.column()), (Some(4), Some(8)));
}

#[test]
fn readers_files_and_slices() {
    let input = "namespace ex\nstring S\n";
    let a = Model::from_idl_reader("a.smithy", input.as_bytes()).unwrap();
    let b = Model::from_idl_slice("b.smithy", input.as_bytes()).unwrap();
    assert!(a.equivalent(&b));
    let path =
        std::env::temp_dir().join(format!("aws-smithy-lang-idl-{}.smithy", std::process::id()));
    std::fs::write(&path, input).unwrap();
    let c = Model::from_idl_file(&path);
    std::fs::remove_file(&path).unwrap();
    assert!(c.unwrap().equivalent(&a));
    assert!(Model::from_idl_file("/nonexistent/model.smithy")
        .unwrap_err()
        .is_io());
}
