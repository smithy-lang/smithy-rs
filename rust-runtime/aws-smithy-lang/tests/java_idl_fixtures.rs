/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Agreement with Java Smithy 1.74.1 on checked-in IDL fixtures. See `fixtures/java/README.md`.

use aws_smithy_lang::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use aws_smithy_lang::validation::Validated;
use aws_smithy_lang::Model;
use std::collections::HashSet;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java")
}

/// The location and message of Java's first ERROR event.
struct FirstError {
    line: u64,
    column: u64,
    message: String,
}

/// A fixture's recorded Java outcome.
struct Outcome {
    /// `GROUP/STEM`.
    name: String,
    accepted: bool,
    /// Java validation event IDs with ERROR severity.
    errors: Vec<String>,
    /// Java validation event IDs with DANGER severity.
    dangers: Vec<String>,
    first_error: Option<FirstError>,
}

impl Outcome {
    fn group(&self) -> &str {
        self.name.split('/').next().unwrap()
    }

    fn stem(&self) -> &str {
        self.name.split('/').nth(1).unwrap()
    }
}

fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect()
}

fn outcomes() -> Vec<Outcome> {
    let text = std::fs::read_to_string(fixtures().join("expected/idl/outcomes.json")).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    document["outcomes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, outcome)| Outcome {
            name: name.clone(),
            accepted: outcome["accepted"].as_bool().unwrap(),
            errors: strings(&outcome["errors"]),
            dangers: strings(&outcome["dangers"]),
            first_error: outcome.get("firstError").map(|first| FirstError {
                line: first["line"].as_u64().unwrap(),
                column: first["column"].as_u64().unwrap(),
                message: first["message"].as_str().unwrap().to_owned(),
            }),
        })
        .collect()
}

/// The path of a fixture: a `.smithy` file, or a directory of `.smithy` and `.json` files.
fn fixture_path(name: &str) -> PathBuf {
    let directory = fixtures().join("idl").join(name);
    if directory.is_dir() {
        directory
    } else {
        fixtures().join("idl").join(format!("{name}.smithy"))
    }
}

/// Assembles a fixture the way Java's `addImport` does.
fn assemble(name: &str) -> Validated<Model> {
    let path = fixture_path(name);
    let mut assembler = Model::assembler();
    if path.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(&path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        for file in files {
            assembler = match file.extension().and_then(|e| e.to_str()) {
                Some("smithy") => assembler.add_idl_file(&file),
                Some("json") => assembler.add_json_file(&file),
                _ => panic!("{name}: unexpected fixture file {}", file.display()),
            };
        }
    } else {
        assembler = assembler.add_idl_file(&path);
    }
    assembler.assemble()
}

fn load_java(stem: &str) -> Model {
    Model::from_json_file(fixtures().join(format!("expected/idl/{stem}.json")))
        .unwrap_or_else(|e| panic!("{stem}: Java output does not load: {:?}", e.diagnostics()))
}

fn codes<'a>(diagnostics: impl IntoIterator<Item = &'a Diagnostic>) -> HashSet<DiagnosticCode> {
    diagnostics.into_iter().map(|d| d.code()).collect()
}

/// The Rust diagnostic code for a Java validation event ID on a rejected fixture.
///
/// Java reports syntax and most loader problems under the generic `Model` event, so that ID is
/// resolved per fixture. Unknown IDs fail the test so that regenerated fixtures cannot silently
/// lose their reason check.
fn rust_code(fixture: &str, java_event: &str) -> DiagnosticCode {
    use DiagnosticCode::*;
    match java_event {
        "Target.UnresolvedShape" => UnresolvedReference,
        "Model.UnresolvedTrait" => UnresolvedReference,
        // A mixin used other than as a mixin, or a shape without `@mixin` used as one.
        "Target" => InvalidTarget,
        // A mixin applied as a trait.
        "TraitTarget" => InvalidTrait,
        "Model" => match fixture {
            "syntax_duplicate_control"
            | "syntax_duplicate_member"
            | "syntax_duplicate_metadata_key"
            | "syntax_duplicate_shape"
            | "syntax_use_conflict" => DuplicateKey,
            // An empty IDL enum is a syntax error here; Java reports it while building shapes.
            "syntax_empty_enum" => IdlSyntax,
            // `set` is recognized only to explain that it is unsupported.
            "syntax_set_shape" => UnsupportedFeature,
            "missing_elided_identifier" => InvalidProperty,
            // Two definitions of a shape whose `= value` assignments differ.
            "merge_conflicting_value_assignment" => ShapeConflict,
            // Two definitions that apply different values of a non-list trait.
            "merge_conflicting_trait" => TraitConflict,
            // An `apply` with a value that conflicts with the shape's own trait.
            "apply_conflict" => TraitConflict,
            // An `apply` to a member only a later, conflicting definition has: only the conflict
            // is reported, not the trait.
            "apply_json_member_of_later_definition"
            | "apply_json_held_member_of_later_definition" => ShapeConflict,
            // Traits applied to a shape or member that nothing defines. Prelude shapes are not
            // among the shapes an `apply` can target, even from the prelude namespace.
            "apply_unknown_target"
            | "apply_json_unknown_member"
            | "apply_json_unknown_member_pending"
            | "apply_in_prelude_namespace"
            | "apply_json_prelude_shape" => UnknownShape,
            // An IDL `apply` outside the prelude namespace to a shape in it.
            "apply_prelude_shape" | "apply_prelude_namespace_shape" => PreludeConflict,
            "unsupported_version" => UnsupportedVersion,
            // Two definitions of a shape with different mixins or members.
            "loader_mixins_conflict_error"
            | "focused_mixins_different_definitions"
            | "focused_mixins_duplicate_mixin_order"
            | "focused_mixins_elided_default_sugar_conflict" => ShapeConflict,
            // Mixin cycles, and members inherited or redefined with different targets.
            "loader_mixins_detects_cycles1"
            | "loader_mixins_detects_cycles2"
            | "loader_mixins_detects_cycles3"
            | "focused_mixins_cycle_dependents"
            | "validators_mixins_illegal_redefined_member"
            | "focused_mixins_elision_conflict"
            | "focused_mixins_member_conflict"
            | "focused_mixins_member_conflict_applied" => InvalidShape,
            // A missing mixin, and the shapes that use the shapes that use it.
            "loader_mixins_detects_missing_transitive" | "focused_mixins_missing" => {
                UnresolvedReference
            }
            // Mixins of another shape type.
            "focused_mixins_type_mismatch" => InvalidTarget,
            // A `$name` member that no bound resource or mixin provides.
            "focused_mixins_elision_unknown" | "focused_mixins_duplicate_elided_unknown" => {
                InvalidProperty
            }
            // A trait applied to a member that a shape neither declares nor inherits.
            "focused_mixins_apply_unknown_member" => UnknownShape,
            // An enum without members and without mixins is a syntax error here, because the
            // grammar requires a member; Java reports it while building shapes.
            "validators_mixins_empty_enum_mixin" => IdlSyntax,
            other if other.starts_with("syntax_") => IdlSyntax,
            other => panic!("{other}: map Java's generic `Model` event to a Rust code"),
        },
        other => panic!("{fixture}: unmapped Java event `{other}`; add it to `rust_code`"),
    }
}

/// Codes this crate reports in addition to [`rust_code`]'s, for fixtures where one Java event ID
/// covers errors that have different codes here.
fn additional_rust_codes(fixture: &str) -> &'static [DiagnosticCode] {
    match fixture {
        // A shape that cannot be built, then the shape that uses it as a mixin, which is missing.
        "focused_mixins_type_mismatch" | "focused_mixins_member_conflict" => {
            &[DiagnosticCode::UnresolvedReference]
        }
        _ => &[],
    }
}

/// The Rust diagnostic code for a fixture Java accepts but this crate deliberately rejects.
fn intentional_rejection(fixture: &str) -> DiagnosticCode {
    use DiagnosticCode::*;
    match fixture {
        // Mixins of shape types other than structure, union, enum, and intEnum.
        "spec_mixins" => UnsupportedFeature,
        // A shape that is its own mixin, which the reference implementation drops without a
        // diagnostic, with the shapes that use it.
        "focused_mixins_self_reference" => InvalidShape,
        // Unknown properties are errors here; the reference implementation warns and applies
        // the traits.
        "apply_extra_property" => UnknownProperty,
        "version_1" => UnsupportedVersion,
        // The IDL grammar forbids leading zeroes; Java's tokenizer accepts them.
        "strict_leading_zero" => IdlSyntax,
        // The grammar requires spaces after `apply`; the reference implementation also accepts a
        // line break or a comment there.
        "apply_newline_after_keyword" | "apply_comment_after_keyword" => IdlSyntax,
        other => panic!("{other}: document why this crate rejects a model Java accepts"),
    }
}

/// Java syntax-error columns that differ from this crate's, as `(fixture, rust_column)`.
///
/// Java reports these at the start of the token it was reading (or, for a duplicate control
/// statement, after its value); this crate points at the offending character or name.
const COLUMN_DIFFERENCES: &[(&str, u64)] = &[
    // The duplicated name, not the end of the statement.
    ("syntax_duplicate_control", 2),
    // The escape, not the start of the string.
    ("syntax_invalid_escape", 19),
    // Where an exponent digit is expected, not the start of the number.
    ("syntax_missing_exponent_digits", 16),
    // Where the opening line must end, not the start of the text block.
    ("syntax_text_block_same_line", 19),
    ("syntax_text_block_space_only", 20),
    // The start of the version value, not its end.
    ("unsupported_version", 11),
];

#[test]
fn every_fixture_has_an_outcome() {
    let recorded: Vec<_> = outcomes().into_iter().map(|o| o.name).collect();
    let mut on_disk = Vec::new();
    for group in ["valid", "danger", "unsupported", "invalid"] {
        for entry in std::fs::read_dir(fixtures().join("idl").join(group)).unwrap() {
            let path = entry.unwrap().path();
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            on_disk.push(format!("{group}/{stem}"));
        }
    }
    on_disk.sort();
    let mut recorded_sorted = recorded.clone();
    recorded_sorted.sort();
    assert_eq!(
        recorded_sorted, on_disk,
        "regenerate expected/ (see README.md)"
    );
}

#[test]
fn accepted_fixtures_match_java() {
    let mut failures = Vec::new();
    for outcome in outcomes() {
        let accepts = matches!(outcome.group(), "valid" | "danger");
        if !accepts {
            continue;
        }
        let name = &outcome.name;
        assert!(
            outcome.accepted && outcome.errors.is_empty(),
            "{name}: Java rejects"
        );
        let validated = assemble(name);
        let diagnostics: Vec<_> = validated.diagnostics().iter().cloned().collect();
        let model = match validated.into_result() {
            Ok(model) => model,
            Err(e) => {
                failures.push(format!("{name}: Rust rejects: {:?}", e.diagnostics()));
                continue;
            }
        };
        let java = load_java(outcome.stem());
        if !model.equivalent(&java) {
            let writer = aws_smithy_lang::AstWriter::new()
                .canonical(true)
                .pretty(true);
            failures.push(format!(
                "{name}: Rust and Java models differ\n--- rust\n{}\n--- java\n{}",
                writer.to_string(&model).unwrap(),
                writer.to_string(&java).unwrap()
            ));
            continue;
        }

        // A Java DANGER `SyntacticShapeIdTarget` is a Rust warning; nothing else is reported.
        let warned: HashSet<_> = diagnostics
            .iter()
            .filter(|d| d.code() == DiagnosticCode::SyntacticShapeIdTarget)
            .inspect(|d| assert_eq!(d.severity(), Severity::Warning, "{name}"))
            .map(|d| d.code())
            .collect();
        let expected: HashSet<_> = outcome
            .dangers
            .iter()
            .map(|id| match id.as_str() {
                "SyntacticShapeIdTarget" => DiagnosticCode::SyntacticShapeIdTarget,
                other => panic!("{name}: unmapped Java DANGER event `{other}`"),
            })
            .collect();
        if warned != expected {
            failures.push(format!(
                "{name}: expected warnings {expected:?}: {diagnostics:?}"
            ));
        }
        assert_eq!(
            outcome.group() == "danger",
            !expected.is_empty(),
            "{name}: wrong group"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn rejected_fixtures_match_java() {
    let mut failures = Vec::new();
    for outcome in outcomes() {
        if outcome.group() != "invalid" {
            continue;
        }
        let name = &outcome.name;
        assert!(!outcome.accepted, "{name}: Java accepts");
        let Err(err) = assemble(name).into_result() else {
            failures.push(format!("{name}: Java rejects but Rust accepts"));
            continue;
        };
        let expected: HashSet<_> = outcome
            .errors
            .iter()
            .map(|e| rust_code(outcome.stem(), e))
            .chain(additional_rust_codes(outcome.stem()).iter().copied())
            .collect();
        // The recorded outcomes list only ERROR events; notes and warnings may accompany them.
        let errors = err
            .diagnostics()
            .iter()
            .filter(|d| d.severity() == Severity::Error);
        if codes(errors) != expected {
            failures.push(format!(
                "{name}: Java events {:?}, Rust diagnostics {:?}",
                outcome.errors,
                err.diagnostics()
            ));
            continue;
        }

        // Syntax errors are reported at the same position as Java's.
        let first = outcome.first_error.as_ref().unwrap();
        if first.message.starts_with("Syntax error") {
            let diagnostic = err.diagnostics().iter().next().unwrap();
            let location = diagnostic.source_location().unwrap();
            let column = COLUMN_DIFFERENCES
                .iter()
                .find(|(fixture, _)| *fixture == outcome.stem())
                .map_or(first.column, |(_, column)| *column);
            if (location.line(), location.column())
                != (Some(first.line as u32), Some(column as u32))
            {
                failures.push(format!(
                    "{name}: Java reports `{}`, Rust reports `{diagnostic}`",
                    first.message
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn intentional_differences_are_rejected_precisely() {
    for outcome in outcomes() {
        if outcome.group() != "unsupported" {
            continue;
        }
        let name = &outcome.name;
        assert!(
            outcome.accepted,
            "{name}: Java rejects; move it to `invalid/`"
        );
        let err = assemble(name)
            .into_result()
            .expect_err(&format!("{name}: Rust accepts"));
        let expected = intentional_rejection(outcome.stem());
        assert!(
            !err.diagnostics().is_empty() && err.diagnostics().iter().all(|d| d.code() == expected),
            "{name}: expected only {expected:?}, got {:?}",
            err.diagnostics()
        );
    }
}

/// How this crate resolved mixins, in the form of `expected/idl/NAME.mixins.json` (see
/// `Regenerate.java`), with an enum member's implicit value written out like the reference
/// implementation does.
fn mixin_resolution(model: &Model) -> serde_json::Value {
    use aws_smithy_lang::shape::AppliedTrait;
    use serde_json::{json, Map, Value};
    fn traits<'a>(traits: impl Iterator<Item = AppliedTrait<'a>>) -> Value {
        let traits: Map<String, Value> = traits
            .map(|t| {
                let value = serde_json::from_str(&t.value().to_json_string()).unwrap();
                (t.id().to_string(), value)
            })
            .collect();
        Value::Object(traits)
    }
    fn introduced<'a>(traits: impl Iterator<Item = AppliedTrait<'a>>) -> Value {
        let mut ids: Vec<String> = traits.map(|t| t.id().to_string()).collect();
        ids.sort();
        json!(ids)
    }
    let mut shapes = Map::new();
    for shape in model.non_prelude_shapes() {
        if shape.mixins().len() == 0 {
            continue;
        }
        let members: Vec<Value> = shape
            .members()
            .iter()
            .map(|member| {
                let mut resolved = json!({
                    "name": member.name(),
                    "target": member.target().as_str(),
                    "mixins": member.mixins().map(|m| m.id().to_string()).collect::<Vec<_>>(),
                    "traits": traits(member.traits().iter()),
                    "introduced": introduced(member.introduced_traits()),
                });
                let value = aws_smithy_lang::traits::ENUM_VALUE.to_string();
                if shape.is_enum() && !member.has_trait(&aws_smithy_lang::traits::ENUM_VALUE) {
                    resolved["traits"][&value] = json!(member.name());
                    if !member.is_inherited() {
                        resolved["introduced"] = json!([value]);
                    }
                }
                resolved
            })
            .collect();
        shapes.insert(
            shape.id().to_string(),
            json!({
                "mixins": shape.mixins().map(|m| m.id().to_string()).collect::<Vec<_>>(),
                "traits": traits(shape.traits().iter()),
                "introduced": introduced(shape.introduced_traits()),
                "members": members,
            }),
        );
    }
    Value::Object(shapes)
}

#[test]
fn mixins_resolve_like_reference_impl() {
    // The serialized models do not show this: they keep mixins in their declared form, which this
    // crate resolves the same way when it loads either side.
    let mut failures = Vec::new();
    let mut checked = 0;
    for outcome in outcomes() {
        if !matches!(outcome.group(), "valid" | "danger") {
            continue;
        }
        let model = assemble(&outcome.name).into_result().unwrap();
        let mut rust = mixin_resolution(&model);
        let path = fixtures().join(format!("expected/idl/{}.mixins.json", outcome.stem()));
        let Ok(text) = std::fs::read_to_string(&path) else {
            // `Regenerate.java` records every accepted fixture with shapes that use mixins.
            if rust.as_object().is_some_and(|shapes| !shapes.is_empty()) {
                failures.push(format!(
                    "{}: regenerate expected/ (see README.md)",
                    outcome.name
                ));
            }
            continue;
        };
        checked += 1;
        let mut reference: serde_json::Value = serde_json::from_str(&text).unwrap();
        // The reference implementation also counts a member's traits from its second and later
        // mixins as its own (see the README), so own traits are compared only for members with
        // one mixin.
        for side in [&mut rust, &mut reference] {
            for shape in side.as_object_mut().unwrap().values_mut() {
                for member in shape["members"].as_array_mut().unwrap() {
                    if member["mixins"].as_array().unwrap().len() > 1 {
                        member.as_object_mut().unwrap().remove("introduced");
                    }
                }
            }
        }
        if rust != reference {
            failures.push(format!(
                "{}: mixins resolve differently\n--- rust\n{}\n--- reference\n{}",
                outcome.name,
                serde_json::to_string_pretty(&rust).unwrap(),
                serde_json::to_string_pretty(&reference).unwrap()
            ));
        }
    }
    assert!(
        checked > 0,
        "no fixture records how the reference implementation resolves mixins"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn idl_fixtures_round_trip_through_the_json_ast() {
    // Writing an IDL-loaded model and reading it back as JSON preserves it.
    for outcome in outcomes() {
        if !matches!(outcome.group(), "valid" | "danger") {
            continue;
        }
        let model = assemble(&outcome.name).into_result().unwrap();
        // Like the reference implementation's serializer, the writer leaves out every shape in
        // the prelude namespace unless the prelude is included, even shapes a fixture adds.
        let adds_prelude_shapes = model
            .non_prelude_shapes()
            .any(|shape| aws_smithy_lang::prelude::is_prelude_shape(shape.id()));
        let json = aws_smithy_lang::AstWriter::new()
            .include_prelude(adds_prelude_shapes)
            .to_string(&model)
            .unwrap();
        let reloaded = Model::from_json_str("written.json", &json)
            .unwrap_or_else(|e| panic!("{}: {:?}", outcome.name, e.diagnostics()));
        assert!(model.equivalent(&reloaded), "{}", outcome.name);
    }
}
