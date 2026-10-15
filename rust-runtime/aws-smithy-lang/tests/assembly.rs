/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Model assembly: combining inputs, merge rules, shapes built in code, custom validators, and
//! source locations.

use aws_smithy_lang::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use aws_smithy_lang::shape::{
    EnumShape, IntEnumShape, ListShape, MapShape, MemberShape, OperationShape, ResourceShape,
    ServiceShape, ShapeType, SimpleShape, StructureShape, UnionShape,
};
use aws_smithy_lang::traits::{self, Documentation};
use aws_smithy_lang::{
    validation, AstWriter, Model, ModelAssembler, Node, NodeObject, ShapeId, SourceLocation,
    Validated,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn doc(shapes: &str) -> String {
    format!(r#"{{"smithy":"2.0","shapes":{{{shapes}}}}}"#)
}

fn codes(validated: &Validated<Model>) -> Vec<(Severity, DiagnosticCode)> {
    validated
        .diagnostics()
        .iter()
        .map(|d| (d.severity(), d.code()))
        .collect()
}

fn expect_model(validated: Validated<Model>) -> Model {
    let diagnostics: Vec<_> = validated
        .diagnostics()
        .iter()
        .map(|d| d.to_string())
        .collect();
    validated
        .into_result()
        .unwrap_or_else(|e| panic!("{e}\n{}", diagnostics.join("\n")))
}

fn empty() -> NodeObject {
    NodeObject::new()
}

/// A model containing every shape kind, built entirely in code.
fn built_in_code() -> Model {
    let shapes: Vec<aws_smithy_lang::Shape> = vec![
        ServiceShape::builder("ex#Weather")
            .version("2006-03-01")
            .add_resource("ex#City")
            .add_operation("ex#GetCurrentTime")
            .add_error("ex#Throttled")
            .rename("ex#CityId", "Id")
            .add_trait(traits::DOCUMENTATION, "The weather service.")
            .build()
            .unwrap()
            .into(),
        ResourceShape::builder("ex#City")
            .identifier("cityId", "ex#CityId")
            .property("name", "smithy.api#String")
            .read("ex#GetCity")
            .list("ex#ListCities")
            .add_resource("ex#Forecast")
            .build()
            .unwrap()
            .into(),
        ResourceShape::builder("ex#Forecast")
            .identifier("cityId", "ex#CityId")
            .read("ex#GetForecast")
            .build()
            .unwrap()
            .into(),
        OperationShape::builder("ex#GetCity")
            .input("ex#GetCityInput")
            .output("ex#GetCityOutput")
            .add_error("ex#NoSuchResource")
            .add_trait("smithy.api#readonly", empty())
            .build()
            .unwrap()
            .into(),
        OperationShape::builder("ex#ListCities")
            .build()
            .unwrap()
            .into(),
        OperationShape::builder("ex#GetForecast")
            .build()
            .unwrap()
            .into(),
        OperationShape::builder("ex#GetCurrentTime")
            .build()
            .unwrap()
            .into(),
        StructureShape::builder("ex#GetCityInput")
            .add_member(
                MemberShape::builder("ex#GetCityInput$cityId", "ex#CityId")
                    .add_trait(traits::REQUIRED, empty())
                    .build()
                    .unwrap(),
            )
            .add_trait(traits::INPUT, empty())
            .build()
            .unwrap()
            .into(),
        StructureShape::builder("ex#GetCityOutput")
            .member("name", "smithy.api#String")
            .member("coordinates", "ex#Coordinates")
            .member("tags", "ex#Tags")
            .member("aliases", "ex#Aliases")
            .member("kind", "ex#Kind")
            .member("rank", "ex#Rank")
            .member("photo", "ex#Photo")
            .add_trait(traits::OUTPUT, empty())
            .build()
            .unwrap()
            .into(),
        StructureShape::builder("ex#NoSuchResource")
            .member("resourceType", "smithy.api#String")
            .add_trait(traits::ERROR, "client")
            .build()
            .unwrap()
            .into(),
        StructureShape::builder("ex#Throttled")
            .add_trait(traits::ERROR, "server")
            .build()
            .unwrap()
            .into(),
        StructureShape::builder("ex#Coordinates")
            .member("latitude", "smithy.api#Float")
            .member("longitude", "smithy.api#Float")
            .build()
            .unwrap()
            .into(),
        MapShape::builder("ex#Tags")
            .key("smithy.api#String")
            .value("smithy.api#String")
            .build()
            .unwrap()
            .into(),
        ListShape::builder("ex#Aliases")
            .member("smithy.api#String")
            .build()
            .unwrap()
            .into(),
        EnumShape::builder("ex#Kind")
            .variant("CITY")
            .enum_value("TOWN", "town")
            .build()
            .unwrap()
            .into(),
        IntEnumShape::builder("ex#Rank")
            .enum_value("FIRST", 1)
            .enum_value("SECOND", 2)
            .build()
            .unwrap()
            .into(),
        UnionShape::builder("ex#Photo")
            .member("url", "smithy.api#String")
            .member("bytes", "smithy.api#Blob")
            .build()
            .unwrap()
            .into(),
        SimpleShape::builder(ShapeType::String, "ex#CityId")
            .add_trait("smithy.api#pattern", "^[A-Za-z0-9 ]+$")
            .build()
            .unwrap()
            .into(),
    ];
    expect_model(Model::assembler().add_shapes(shapes).assemble())
}

#[test]
fn every_shape_kind_can_be_built_in_code() {
    let model = built_in_code();
    let service = model
        .expect_shape("ex#Weather")
        .unwrap()
        .expect_service()
        .unwrap();
    let operations: Vec<_> = service
        .contained_operations()
        .map(|op| op.id().to_string())
        .collect();
    assert_eq!(
        operations,
        [
            "ex#GetCity",
            "ex#GetCurrentTime",
            "ex#GetForecast",
            "ex#ListCities"
        ]
    );
    assert_eq!(
        service.get_trait_as::<Documentation>().unwrap(),
        Some(Documentation("The weather service.".into()))
    );
    let output = model.expect_shape("ex#GetCityOutput").unwrap();
    let names: Vec<_> = output.members().names().collect();
    assert_eq!(
        names,
        [
            "name",
            "coordinates",
            "tags",
            "aliases",
            "kind",
            "rank",
            "photo"
        ]
    );
    let member = model.expect_shape("ex#GetCityInput$cityId").unwrap();
    assert!(member.has_trait("required"));
    // Shapes built in code have no source location.
    assert!(member.source_location().is_none());
    assert!(output.source_location().is_none());

    let mut kinds: Vec<_> = model.non_prelude_shapes().map(|s| s.shape_type()).collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), 10, "{kinds:?}");

    // Writing and reloading the model gives an equivalent model, in insertion order.
    let json = AstWriter::new().to_string(&model).unwrap();
    assert!(
        json.starts_with(r#"{"smithy":"2.0","shapes":{"ex#Weather""#),
        "{json}"
    );
    let reloaded = Model::from_json_str("weather.json", &json).unwrap();
    assert!(reloaded.equivalent(&model));
    assert_eq!(AstWriter::new().to_string(&reloaded).unwrap(), json);
    assert_eq!(
        reloaded
            .expect_shape("ex#City")
            .unwrap()
            .source_location()
            .pointer(),
        Some("/shapes/ex#City")
    );
}

#[test]
fn shapes_from_a_model_can_be_copied_and_modified() {
    let model = built_in_code();
    let structure = model
        .expect_shape("ex#Coordinates")
        .unwrap()
        .expect_structure()
        .unwrap();
    let modified = structure
        .shape()
        .to_builder()
        .member("altitude", "smithy.api#Float")
        .build()
        .unwrap();
    let rebuilt = expect_model(
        Model::assembler()
            .add_shapes(
                model
                    .non_prelude_shapes()
                    .filter(|s| s.id() != "ex#Coordinates")
                    .map(|s| s.shape().clone()),
            )
            .add_shape(modified)
            .assemble(),
    );
    assert_eq!(
        rebuilt
            .expect_shape("ex#Coordinates")
            .unwrap()
            .members()
            .len(),
        3
    );
    assert!(!rebuilt.equivalent(&model));
}

#[test]
fn inputs_combine_and_resolve_across_sources() {
    let operations = doc(
        r#""ex#GetThing":{"type":"operation","input":{"target":"ex#GetThingInput"},
                          "errors":[{"target":"ex#NotFound"}]}"#,
    );
    let errors =
        doc(r#""ex#NotFound":{"type":"structure","traits":{"smithy.api#error":"client"}}"#);
    let service = Model::from_json_str(
        "service.json",
        &doc(
            r#""ex#Service":{"type":"service","operations":[{"target":"ex#Ping"}]},
               "ex#Ping":{"type":"operation"}"#,
        ),
    )
    .unwrap();
    let input = StructureShape::builder("ex#GetThingInput")
        .member("id", "ex#Id")
        .build()
        .unwrap();
    let model = expect_model(
        Model::assembler()
            .add_json_str("operations.json", operations)
            .add_json_slice("errors.json", errors.into_bytes())
            .add_json_reader("ids.json", doc(r#""ex#Id":{"type":"string"}"#).as_bytes())
            .add_model(&service)
            .add_shape(input)
            .assemble(),
    );
    // `add_model` copies shapes, not the other model's prelude; this model uses the real one.
    assert!(model.has_prelude());
    assert_eq!(model.non_prelude_shapes().len(), 6);
    assert!(model.contains_shape("smithy.api#String"));
    let sources: Vec<_> = model
        .non_prelude_shapes()
        .map(|s| s.source_location().source().to_owned())
        .collect();
    assert_eq!(
        sources,
        [
            "operations.json",
            "errors.json",
            "ids.json",
            "service.json",
            "service.json",
            ""
        ]
    );
    let service = model
        .expect_shape("ex#Service")
        .unwrap()
        .expect_service()
        .unwrap();
    assert_eq!(service.contained_operations().len(), 1);
}

#[test]
fn prelude_shapes_cannot_be_redefined() {
    let json = doc(r#""smithy.api#String":{"type":"string"}"#);
    let validated = Model::assembler()
        .add_json_str("p.json", json.clone())
        .assemble();
    assert_eq!(
        codes(&validated),
        [(Severity::Error, DiagnosticCode::PreludeConflict)]
    );
    let built = SimpleShape::builder(ShapeType::String, "smithy.api#String")
        .build()
        .unwrap();
    let validated = Model::assembler().add_shape(built).assemble();
    assert!(validated.has_errors());
    // Without the prelude, `smithy.api` shapes can be defined.
    let model = expect_model(
        Model::assembler()
            .disable_prelude()
            .add_json_str("p", json)
            .assemble(),
    );
    let string = model
        .expect_shape("smithy.api#String")
        .unwrap()
        .expect_shape()
        .unwrap();
    assert!(!string.is_prelude());
    assert!(aws_smithy_lang::prelude::is_prelude_shape(string.id()));
}

#[test]
fn duplicate_shapes_merge_or_conflict() {
    let a = doc(r#""ex#A":{"type":"structure","members":{"x":{"target":"smithy.api#String"}}}"#);
    // Equal definitions from different sources: kept once, with a note.
    let validated = Model::assembler()
        .add_json_str("one.json", a.clone())
        .add_json_str("two.json", a.clone())
        .assemble();
    assert_eq!(
        codes(&validated),
        [(Severity::Note, DiagnosticCode::DuplicateDefinition)]
    );
    let note = validated.diagnostics().iter().next().unwrap();
    assert_eq!(note.source_location().unwrap().source(), "two.json");
    assert_eq!(note.related()[0].source(), "one.json");
    let (model, diagnostics) = validated.into_parts();
    assert_eq!(model.unwrap().non_prelude_shapes().len(), 1);
    assert_eq!(diagnostics.len(), 1);

    // The same model added twice has identical locations: no note at all.
    let model = Model::from_json_str("one.json", &a).unwrap();
    let validated = Model::assembler()
        .add_model(&model)
        .add_model(&model)
        .assemble();
    assert!(validated.diagnostics().is_empty());

    // Equal to a shape built in code.
    let built = StructureShape::builder("ex#A")
        .member("x", "smithy.api#String")
        .build()
        .unwrap();
    let validated = Model::assembler()
        .add_json_str("one.json", a.clone())
        .add_shape(built)
        .assemble();
    assert!(!validated.has_errors());

    // Different definitions conflict, naming both locations.
    let b = doc(r#""ex#A":{"type":"structure","members":{"x":{"target":"smithy.api#Integer"}}}"#);
    let validated = Model::assembler()
        .add_json_str("one.json", a)
        .add_json_str("two.json", b)
        .assemble();
    assert_eq!(
        codes(&validated),
        [(Severity::Error, DiagnosticCode::ShapeConflict)]
    );
    let err = validated.into_result().unwrap_err();
    assert!(err.is_invalid_model());
    let conflict = err.diagnostics().iter().next().unwrap();
    assert_eq!(conflict.shape_id().unwrap(), "ex#A");
    assert_eq!(
        conflict.source_location().unwrap().to_string(),
        "two.json#/shapes/ex#A"
    );
    assert_eq!(conflict.related()[0].to_string(), "one.json#/shapes/ex#A");
}

#[test]
fn metadata_merges_like_java_smithy() {
    let m = |json: &str| format!(r#"{{"smithy":"2.0","metadata":{json}}}"#);
    let model = expect_model(
        Model::assembler()
            .add_json_str(
                "a.json",
                m(r#"{"list":[1,2],"same":{"x":1},"only_a":true}"#),
            )
            .add_json_str("b.json", m(r#"{"list":[3],"same":{"x":1}}"#))
            .put_metadata("list", Node::from(vec![Node::from(4i64)]))
            .put_metadata("only_code", "x")
            .assemble(),
    );
    assert_eq!(
        model.metadata_value("list").unwrap().to_json_string(),
        "[1,2,3,4]"
    );
    assert_eq!(
        model.metadata_value("same").unwrap().to_json_string(),
        r#"{"x":1}"#
    );
    let keys: Vec<_> = model.metadata().keys().collect();
    assert_eq!(keys, ["list", "same", "only_a", "only_code"]);
    // Elements keep the locations of the documents they came from.
    let list = model.metadata_value("list").unwrap().as_array().unwrap();
    assert_eq!(
        list[0].source_location().to_string(),
        "a.json#/metadata/list/0"
    );
    assert_eq!(
        list[2].source_location().to_string(),
        "b.json#/metadata/list/0"
    );
    assert!(list[3].source_location().is_none());

    let validated = Model::assembler()
        .add_json_str("a.json", m(r#"{"k":"a"}"#))
        .add_json_str("b.json", m(r#"{"k":"b"}"#))
        .assemble();
    assert_eq!(
        codes(&validated),
        [(Severity::Error, DiagnosticCode::MetadataConflict)]
    );
    let conflict = validated.diagnostics().iter().next().unwrap();
    assert_eq!(
        conflict.source_location().unwrap().to_string(),
        "b.json#/metadata/k"
    );
    assert_eq!(conflict.related()[0].to_string(), "a.json#/metadata/k");
}

#[test]
fn applied_traits_merge_like_java_smithy() {
    let shapes = doc(
        r#""ex#A":{"type":"structure","members":{"m":{"target":"smithy.api#String"}},
                   "traits":{"ex#tags":["a"],"smithy.api#documentation":"doc"}}"#,
    );
    let model = expect_model(
        Model::assembler()
            .add_json_str("a.json", shapes.clone())
            .add_trait("ex#A", "ex#tags", Node::from(vec![Node::from("b")]))
            .add_trait("ex#A", traits::DOCUMENTATION, "doc")
            .add_trait("ex#A$m", traits::REQUIRED, empty())
            .assemble(),
    );
    let a = model.expect_shape("ex#A").unwrap();
    assert_eq!(
        a.get_trait("ex#tags").unwrap().to_json_string(),
        r#"["a","b"]"#
    );
    assert!(model
        .expect_shape("ex#A$m")
        .unwrap()
        .has_trait(traits::REQUIRED));

    let fails = |assembler: ModelAssembler| {
        let validated = assembler.assemble();
        assert!(validated.has_errors());
        validated.diagnostics().iter().next().unwrap().code()
    };
    let base = || Model::assembler().add_json_str("a.json", shapes.clone());
    assert_eq!(
        fails(base().add_trait("ex#A", traits::DOCUMENTATION, "other")),
        DiagnosticCode::TraitConflict
    );
    assert_eq!(
        fails(base().add_trait("ex#Missing", traits::REQUIRED, empty())),
        DiagnosticCode::UnknownShape
    );
    assert_eq!(
        fails(base().add_trait("ex#A$missing", traits::REQUIRED, empty())),
        DiagnosticCode::UnknownShape
    );
    assert_eq!(
        fails(base().add_trait("smithy.api#String", traits::DOCUMENTATION, "x")),
        DiagnosticCode::PreludeConflict
    );
    assert_eq!(
        fails(base().add_trait("not an id", traits::DOCUMENTATION, "x")),
        DiagnosticCode::InvalidShapeId
    );
    assert_eq!(
        fails(base().add_trait("ex#A", traits::MIXIN, empty())),
        DiagnosticCode::UnsupportedFeature
    );
}

#[test]
fn mixins_are_rejected_from_every_input() {
    let json = doc(r#""ex#M":{"type":"structure","traits":{"smithy.api#mixin":{}}}"#);
    let validated = Model::assembler().add_json_str("m.json", json).assemble();
    let error = validated.diagnostics().iter().next().unwrap();
    assert_eq!(error.code(), DiagnosticCode::UnsupportedFeature);
    assert_eq!(
        error.source_location().unwrap().pointer(),
        Some("/shapes/ex#M/traits/smithy.api#mixin")
    );
    let built = StructureShape::builder("ex#M")
        .add_trait(traits::MIXIN, empty())
        .build()
        .unwrap();
    let validated = Model::assembler().add_shape(built).assemble();
    assert_eq!(
        validated.diagnostics().iter().next().unwrap().code(),
        DiagnosticCode::UnsupportedFeature
    );
}

#[test]
fn custom_validators_report_with_their_name() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let json = doc(
        r#""ex#Weather":{"type":"operation"},"ex#GetThing":{"type":"operation"},
           "ex#Thing":{"type":"string","traits":{"smithy.api#documentation":"x"}}"#,
    );
    let validated = Model::assembler()
        .add_json_str("m.json", json.clone())
        .add_validator(validation::from_fn("Notes", |model, diagnostics| {
            diagnostics.push(Diagnostic::note(
                DiagnosticCode::Custom,
                format!("{} shapes", model.non_prelude_shapes().len()),
            ));
        }))
        .add_validator(validation::from_fn("Verbs", move |model, diagnostics| {
            counted.fetch_add(1, Ordering::SeqCst);
            for op in model
                .operations()
                .filter(|op| !op.id().name().starts_with("Get"))
            {
                diagnostics.push(
                    Diagnostic::warning(DiagnosticCode::Custom, "operations should start with Get")
                        .with_shape_id(op.id().clone())
                        .with_source_location(op.source_location().clone()),
                );
            }
        }))
        .assemble();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(validated.result().is_some());
    let reported: Vec<_> = validated
        .diagnostics()
        .iter()
        .map(|d| (d.validator().unwrap(), d.severity(), d.message()))
        .collect();
    assert_eq!(
        reported,
        [
            ("Notes", Severity::Note, "3 shapes"),
            (
                "Verbs",
                Severity::Warning,
                "operations should start with Get"
            ),
        ]
    );
    let warning = validated.diagnostics().iter().nth(1).unwrap();
    assert_eq!(warning.shape_id().unwrap(), "ex#Weather");
    assert_eq!(
        warning.to_string(),
        "warning[Custom][Verbs]: operations should start with Get (at m.json#/shapes/ex#Weather)"
    );

    // An error from a custom validator discards the model.
    let validated = Model::assembler()
        .add_json_str("m.json", json)
        .add_validator(validation::from_fn("Strict", |_, diagnostics| {
            diagnostics.push(Diagnostic::error(DiagnosticCode::Custom, "no"));
        }))
        .assemble();
    assert!(validated.result().is_none());
    let err = validated.into_result().unwrap_err();
    assert!(err.is_invalid_model());
    assert_eq!(err.to_string(), "invalid Smithy model (1 error): no");
}

#[test]
fn validators_only_run_after_critical_checks_pass() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let validated = Model::assembler()
        .add_json_str(
            "m.json",
            doc(r#""ex#L":{"type":"list","member":{"target":"ex#Missing"}}"#),
        )
        .add_validator(validation::from_fn("Counter", move |_, _| {
            counted.fetch_add(1, Ordering::SeqCst);
        }))
        .assemble();
    assert_eq!(
        codes(&validated),
        [(Severity::Error, DiagnosticCode::UnresolvedReference)]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn built_in_validators_are_named() {
    let validated = Model::assembler()
        .add_json_str(
            "m.json",
            doc(r#""ex#U":{"type":"union"},"ex#S":{"type":"string","traits":{"ex#S":{}}}"#),
        )
        .assemble();
    let names: Vec<_> = validated
        .diagnostics()
        .iter()
        .map(|d| d.validator().unwrap())
        .collect();
    assert_eq!(names, ["TraitDefinition", "UnionMembers"]);
}

#[test]
fn trait_codec_validator_points_at_nested_values() {
    #[derive(Debug)]
    struct Limits;
    impl traits::TraitCodec for Limits {
        const ID: ShapeId = ShapeId::from_static("ex#limits");
        fn from_node(value: &Node) -> Result<Self, traits::TraitDecodeError> {
            let object = value
                .as_object()
                .ok_or_else(|| traits::TraitDecodeError::at(value, "expected an object"))?;
            for (_, limit) in object.iter() {
                if limit.as_number().and_then(|n| n.as_u64()).is_none() {
                    return Err(traits::TraitDecodeError::at(limit, "expected an integer"));
                }
            }
            Ok(Limits)
        }
        fn to_node(&self) -> Node {
            Node::from(empty())
        }
    }
    let json = doc(
        r#""ex#A":{"type":"string","traits":{"ex#limits":{"min":1,"max":"ten"}}},
           "ex#B":{"type":"string","traits":{"ex#limits":{"min":1}}}"#,
    );
    let model = Model::from_json_str("m.json", &json).unwrap();
    let err = model
        .expect_shape("ex#A")
        .unwrap()
        .get_trait_as::<Limits>()
        .unwrap_err();
    assert_eq!(
        err.source_location().to_string(),
        "m.json#/shapes/ex#A/traits/ex#limits/max"
    );
    let validated = Model::assembler()
        .add_json_str("m.json", json)
        .add_validator(validation::trait_codec::<Limits>())
        .assemble();
    let errors: Vec<_> = validated.diagnostics().errors().collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code(), DiagnosticCode::InvalidTrait);
    assert_eq!(errors[0].validator(), Some("TraitCodec[ex#limits]"));
    assert_eq!(errors[0].shape_id().unwrap(), "ex#A");
    assert_eq!(
        errors[0].source_location().unwrap().pointer(),
        Some("/shapes/ex#A/traits/ex#limits/max")
    );
}

#[test]
fn every_loaded_value_has_a_source_location() {
    let json = r#"{"smithy":"2.0","metadata":{"m":[{"k":true}]},"shapes":{
        "ex#S":{"type":"structure","members":{"a/b":{"target":"smithy.api#String",
               "traits":{"ex#t":{"x":[1,{"y":null}]}}}}}}}"#
        .replace("a/b", "ab");
    let model = Model::from_json_str("m.json", &json).unwrap();
    let shape = model.expect_shape("ex#S").unwrap();
    assert_eq!(shape.source_location().to_string(), "m.json#/shapes/ex#S");
    let member = model.expect_shape("ex#S$ab").unwrap();
    assert_eq!(
        member.source_location().to_string(),
        "m.json#/shapes/ex#S/members/ab"
    );
    let applied = member.traits().iter().next().unwrap();
    assert_eq!(
        applied.source_location().pointer(),
        Some("/shapes/ex#S/members/ab/traits/ex#t")
    );
    let nested = applied.value().pointer("/x/1/y").unwrap();
    assert_eq!(
        nested.source_location().pointer(),
        Some("/shapes/ex#S/members/ab/traits/ex#t/x/1/y")
    );
    let metadata = model.metadata_value("m").unwrap().pointer("/0/k").unwrap();
    assert_eq!(
        metadata.source_location().to_string(),
        "m.json#/metadata/m/0/k"
    );
    // Locations never affect equality or output.
    let built = StructureShape::builder("ex#S")
        .add_member(
            MemberShape::builder("ex#S$ab", "smithy.api#String")
                .add_trait(
                    "ex#t",
                    NodeObject::from_iter([(
                        "x",
                        Node::from(vec![
                            Node::from(1i64),
                            Node::from(NodeObject::from_iter([("y", Node::null())])),
                        ]),
                    )]),
                )
                .build()
                .unwrap(),
        )
        .source_location(SourceLocation::none())
        .build()
        .unwrap();
    assert_eq!(
        shape.expect_shape().unwrap().shape(),
        &aws_smithy_lang::Shape::from(built)
    );
    assert!(!AstWriter::new()
        .to_string(&model)
        .unwrap()
        .contains("m.json"));
}

#[test]
fn limits_and_failures_apply_per_input() {
    let big = doc(r#""ex#A":{"type":"string"},"ex#B":{"type":"string"}"#);
    let validated = Model::assembler()
        .max_shapes(1)
        .add_json_str("small.json", doc(r#""ex#A":{"type":"string"}"#))
        .add_json_str("big.json", big)
        .assemble();
    let err = validated.into_result().unwrap_err();
    assert!(err.is_resource_limit());

    let err = Model::assembler()
        .add_json_str("ok.json", doc(""))
        .add_json_file("/definitely/not/here.json")
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_io());
    use std::error::Error as _;
    assert!(err.source().is_some());

    let err = Model::assembler()
        .add_json_str("bad.json", "{")
        .add_json_str(
            "worse.json",
            r#"{"smithy":"2.0","shapes":{"x":{"type":"string"}}}"#,
        )
        .assemble()
        .into_result()
        .unwrap_err();
    assert!(err.is_json_syntax());
    // Diagnostics from every input are reported.
    let sources: Vec<_> = err
        .diagnostics()
        .iter()
        .map(|d| d.source_location().unwrap().source().to_owned())
        .collect();
    assert_eq!(sources, ["bad.json", "worse.json"]);
}

#[test]
fn public_types_are_send_and_sync() {
    fn assert<T: Send + Sync + 'static>() {}
    assert::<ModelAssembler>();
    assert::<Validated<Model>>();
    assert::<aws_smithy_lang::Shape>();
    assert::<StructureShape>();
    assert::<aws_smithy_lang::shape::StructureShapeBuilder>();
    assert::<Node>();
    assert::<SourceLocation>();
    assert::<Diagnostic>();
    fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
    assert_error::<aws_smithy_lang::shape::ShapeBuildError>();
    assert_error::<traits::TraitDecodeError>();
}
