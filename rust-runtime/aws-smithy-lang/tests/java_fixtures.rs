/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Agreement with Java Smithy 1.74.1 on checked-in fixtures. See `fixtures/java/README.md`.

use aws_smithy_lang::diagnostic::{DiagnosticCode, Severity};
use aws_smithy_lang::waiters::{
    self, AcceptorState, MatcherKind, PathComparator, PathMatcher, Waitable,
};
use aws_smithy_lang::{Model, ShapeId};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java")
}

/// A fixture's recorded Java outcome.
struct Outcome {
    name: String,
    accepted: bool,
    /// Java validation event IDs with ERROR or DANGER severity.
    errors: Vec<String>,
}

fn outcomes() -> Vec<Outcome> {
    let text = std::fs::read_to_string(fixtures().join("expected/outcomes.json")).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    document["outcomes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, outcome)| Outcome {
            name: name.clone(),
            accepted: outcome["accepted"].as_bool().unwrap(),
            errors: outcome["errors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_owned())
                .collect(),
        })
        .collect()
}

/// The Rust diagnostic code corresponding to a Java validation event ID.
///
/// Java reports most loader-level problems under the generic `Model` event, so that ID is
/// resolved per fixture. Unknown IDs fail the test so that regenerated fixtures cannot silently
/// lose their reason check.
fn rust_code(fixture: &str, java_event: &str) -> DiagnosticCode {
    use DiagnosticCode::*;
    match java_event {
        "EnumShape" | "Union" | "ResourceCycle" => InvalidShape,
        "Target" | "UnitType" => InvalidTarget,
        "Target.UnresolvedShape" => UnresolvedReference,
        "ShapeIdConflict" => CaseConflict,
        "Model" => match fixture {
            "invalid/enum_member_not_unit" => InvalidTarget,
            "invalid/invalid_shape_id" => InvalidShapeId,
            "invalid/trait_not_definition" => InvalidTrait,
            "invalid/unknown_shape_type" => UnknownShapeType,
            other => panic!("{other}: map Java's generic `Model` event to a Rust code"),
        },
        other => panic!("{fixture}: unmapped Java event `{other}`; add it to `rust_code`"),
    }
}

fn load(path: &Path) -> Result<Model, aws_smithy_lang::LoadError> {
    Model::from_json_file(path)
}

#[test]
fn every_fixture_has_an_outcome() {
    let recorded: Vec<_> = outcomes().into_iter().map(|o| o.name).collect();
    let mut on_disk = Vec::new();
    for group in ["valid", "invalid"] {
        for entry in std::fs::read_dir(fixtures().join(group)).unwrap() {
            let path = entry.unwrap().path();
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            on_disk.push(format!("{group}/{stem}"));
        }
    }
    on_disk.sort();
    assert_eq!(recorded, on_disk, "regenerate expected/ (see README.md)");
}

#[test]
fn outcomes_match_java() {
    for Outcome {
        name,
        accepted,
        errors,
    } in outcomes()
    {
        assert_eq!(
            name.starts_with("valid/"),
            accepted,
            "{name}: fixture is in the wrong group"
        );
        assert_eq!(accepted, errors.is_empty(), "{name}: inconsistent outcome");
        let result = load(&fixtures().join(format!("{name}.json")));
        match (accepted, result) {
            (true, Err(err)) => panic!(
                "{name}: Java accepts but Rust rejects: {:?}",
                err.diagnostics()
            ),
            (false, Ok(_)) => panic!("{name}: Java rejects but Rust accepts"),
            (false, Err(err)) => {
                // Rust must reject for the same reasons Java does, not merely reject.
                let expected: HashSet<_> = errors.iter().map(|e| rust_code(&name, e)).collect();
                let actual: HashSet<_> = err.diagnostics().iter().map(|d| d.code()).collect();
                assert_eq!(
                    actual,
                    expected,
                    "{name}: Java events {errors:?}, Rust diagnostics {:?}",
                    err.diagnostics()
                );
            }
            (true, Ok(_)) => {}
        }
    }
}

#[test]
fn java_serialization_is_equivalent() {
    for Outcome { name, accepted, .. } in outcomes() {
        if !accepted {
            continue;
        }
        let stem = name.trim_start_matches("valid/");
        let original = load(&fixtures().join(format!("{name}.json"))).unwrap();
        let java = load(&fixtures().join(format!("expected/{stem}.json")))
            .unwrap_or_else(|e| panic!("{name}: Java output does not load: {:?}", e.diagnostics()));
        assert!(
            original.equivalent(&java),
            "{name}: Java output is not equivalent"
        );
    }
}

#[test]
fn all_shape_kinds_fixture_is_complete() {
    let model = load(&fixtures().join("valid/all_shape_kinds.json")).unwrap();
    let mut kinds: Vec<_> = model.non_prelude_shapes().map(|s| s.shape_type()).collect();
    kinds.sort();
    kinds.dedup();
    let mut every: Vec<_> = aws_smithy_lang::ShapeType::ALL
        .iter()
        .copied()
        .filter(|t| *t != aws_smithy_lang::ShapeType::Member)
        .collect();
    every.sort();
    assert_eq!(kinds, every);
}

/// Agreement with Java's `TopDownIndex`, `OperationIndex`, and `BottomUpIndex` on every
/// `knowledge/` fixture.
#[test]
fn knowledge_matches_java() {
    use serde_json::{json, Value};

    fn strings(values: impl IntoIterator<Item = impl ToString>) -> Value {
        Value::from(
            values
                .into_iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>(),
        )
    }

    let dir = fixtures().join("knowledge");
    let mut fixtures_checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_owned();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures().join("expected/knowledge").join(&file))
                .unwrap_or_else(|e| panic!("{}: regenerate expected output: {e}", path.display())),
        )
        .unwrap();
        let model = load(&path).unwrap();
        let knowledge = model.knowledge();
        let name = path.display();

        for (entity, answer) in expected["topDown"].as_object().unwrap() {
            let top_down = knowledge.top_down();
            let entity: ShapeId = entity.parse().unwrap();
            let actual = json!({
                "operations": strings(top_down.get_contained_operations(&entity).unwrap().map(|o| o.id().clone())),
                "resources": strings(top_down.get_contained_resources(&entity).unwrap().map(|r| r.id().clone())),
            });
            assert_eq!(&actual, answer, "{name}: top-down {entity}");
        }

        for (id, answer) in expected["operations"].as_object().unwrap() {
            let operation = knowledge.operations().get(&id.parse().unwrap()).unwrap();
            let meaningful = |s: Option<aws_smithy_lang::shape::StructureView<'_>>| {
                s.map_or(Value::Null, |s| Value::from(s.id().to_string()))
            };
            let mut errors_in = serde_json::Map::new();
            for service in model.services() {
                if let Some(errors) = operation.errors_in(service.id()) {
                    errors_in.insert(
                        service.id().to_string(),
                        strings(errors.map(|e| e.id().clone())),
                    );
                }
            }
            let actual = json!({
                "input": operation.input_shape().id().to_string(),
                "output": operation.output_shape().id().to_string(),
                "meaningfulInput": meaningful(operation.meaningful_input()),
                "meaningfulOutput": meaningful(operation.meaningful_output()),
                "inputMembers": strings(operation.input_members().iter().map(|m| m.id().clone())),
                "outputMembers": strings(operation.output_members().iter().map(|m| m.id().clone())),
                "directErrors": strings(operation.direct_errors().map(|e| e.id().clone())),
                "errorsIn": errors_in,
            });
            assert_eq!(&actual, answer, "{name}: operation {id}");
        }

        for (key, answer) in expected["structures"].as_object().unwrap() {
            let operations = knowledge.operations();
            let id: &ShapeId = &key.parse().unwrap();
            let actual = json!({
                "inputBindings": strings(operations.input_bindings(id).unwrap().map(|o| o.id().clone())),
                "outputBindings": strings(operations.output_bindings(id).unwrap().map(|o| o.id().clone())),
                "errorBindings": strings(operations.error_bindings(id).unwrap().map(|s| s.id().clone())),
                "isInput": operations.is_input_structure(id),
                "isOutput": operations.is_output_structure(id),
            });
            assert_eq!(&actual, answer, "{name}: structure {key}");
        }

        // Java stores one path per entity, overwriting others; compare only unique chains.
        let mut unique = 0;
        let mut ambiguous = 0;
        for (service, entities) in expected["bottomUp"].as_object().unwrap() {
            for (entity, java_chain) in entities.as_object().unwrap() {
                match knowledge
                    .bottom_up()
                    .parent_chain(&service.parse().unwrap(), &entity.parse().unwrap())
                {
                    Ok(chain) => {
                        let chain = chain.unwrap();
                        assert_eq!(
                            &strings(chain.iter().map(|e| e.id().clone())),
                            java_chain,
                            "{name}: bottom-up {service} {entity}"
                        );
                        unique += 1;
                    }
                    Err(_) => {
                        // Java still reports a single (arbitrary) path.
                        assert!(!java_chain.as_array().unwrap().is_empty());
                        ambiguous += 1;
                    }
                }
            }
        }
        assert!(
            unique > 0 && ambiguous > 0,
            "{name}: expected both unique and ambiguous paths"
        );

        // Every service and resource, operation, and structure has a recorded answer.
        assert_eq!(
            expected["topDown"].as_object().unwrap().len(),
            model.services().count() + model.resources().count()
        );
        assert_eq!(
            expected["operations"].as_object().unwrap().len(),
            model.operations().count()
        );
        fixtures_checked += 1;
    }
    assert!(
        fixtures_checked > 0,
        "no knowledge fixtures in {}",
        dir.display()
    );
}

/// Agreement with Java's `PaginatedIndex` for every service and operation of every
/// `paginated/` fixture.
///
/// Java validated these fixtures, so every query must succeed. Rust's errors for invalid
/// pagination, which Java's index reports as missing information instead, are covered by the
/// crate's unit tests.
#[test]
fn pagination_matches_java() {
    use serde_json::{json, Value};

    fn ids<'a>(
        members: impl IntoIterator<Item = &'a aws_smithy_lang::shape::MemberView<'a>>,
    ) -> Value {
        Value::from(
            members
                .into_iter()
                .map(|m| m.id().to_string())
                .collect::<Vec<_>>(),
        )
    }

    let dir = fixtures().join("paginated");
    let mut fixtures_checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.display();
        let stem = path.file_stem().unwrap().to_string_lossy();
        let expected: Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures().join(format!("expected/paginated/{stem}.json")))
                .unwrap_or_else(|e| panic!("{name}: regenerate expected output: {e}")),
        )
        .unwrap();
        let model = match path.extension().and_then(|e| e.to_str()) {
            Some("smithy") => Model::from_idl_file(&path),
            _ => load(&path),
        }
        .unwrap_or_else(|e| panic!("{name}: {:?}", e.diagnostics()));
        let index = model.knowledge().paginated();

        let answers = expected["pagination"].as_object().unwrap();
        assert_eq!(answers.len(), model.services().count(), "{name}: services");
        let (mut paginated, mut unpaginated) = (0, 0);
        for (service, operations) in answers {
            let operations = operations.as_object().unwrap();
            assert_eq!(
                operations.len(),
                model.operations().count(),
                "{name}: operations of {service}"
            );
            for (operation, answer) in operations {
                let info = index
                    .get(&service.parse().unwrap(), &operation.parse().unwrap())
                    .unwrap_or_else(|err| panic!("{name}: {service} {operation}: {err}"));
                let actual = match info {
                    None => {
                        unpaginated += 1;
                        Value::Null
                    }
                    Some(info) => {
                        paginated += 1;
                        let effective = info.paginated_trait();
                        json!({
                            "trait": {
                                "inputToken": effective.input_token(),
                                "outputToken": effective.output_token(),
                                "items": effective.items(),
                                "pageSize": effective.page_size(),
                            },
                            "input": info.input_shape().id().to_string(),
                            "output": info.output_shape().id().to_string(),
                            "inputTokenMember": info.input_token_member().id().to_string(),
                            "outputTokenMemberPath": ids(info.output_token_member_path()),
                            "pageSizeMember": info.page_size_member().map(|m| m.id().to_string()),
                            "itemsMemberPath": ids(info.items_member_path()),
                        })
                    }
                };
                assert_eq!(&actual, answer, "{name}: {service} {operation}");
            }
        }
        assert!(
            paginated > 0 && unpaginated > 0,
            "{name}: expected both paginated and unpaginated pairs"
        );
        fixtures_checked += 1;
    }
    assert!(
        fixtures_checked > 0,
        "no pagination fixtures in {}",
        dir.display()
    );
}

/// The `differences/` waiter fixtures, where this crate deliberately disagrees with Java, and
/// the diagnostics this crate reports for each instead, as `waiter_diagnostics` describes
/// them. Each fixture explains its difference.
const WAITER_DIFFERENCES: &[(&str, &[&str])] = &[
    ("argument_types", &["ex#GetThing Warning InvalidTrait"]),
    (
        "expression_limits",
        &[
            "ex#Long Error ResourceLimit",
            "ex#Nested Error ResourceLimit",
        ],
    ),
    ("function_results", &[]),
    ("projected_message", &[]),
    ("same_operation_names", &["ex#GetThing Error InvalidTrait"]),
    ("unknown_message_type", &[]),
];

/// A waiter fixture and Java's recorded outcome for it.
struct WaiterFixture {
    /// `GROUP/NAME`.
    name: String,
    path: PathBuf,
    expected: serde_json::Value,
}

impl WaiterFixture {
    fn accepted(&self) -> bool {
        self.expected["accepted"].as_bool().unwrap()
    }

    /// Java's ERROR, DANGER, and WARNING events, described as `waiter_diagnostics` describes
    /// Rust's diagnostics. DANGER is a warning, because `Severity` has no DANGER level.
    fn java_diagnostics(&self) -> BTreeSet<String> {
        self.expected["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| {
                let id = event["id"].as_str().unwrap();
                let severity = match event["severity"].as_str().unwrap() {
                    "ERROR" => Severity::Error,
                    "DANGER" | "WARNING" => Severity::Warning,
                    other => panic!("{}: unexpected severity {other}", self.name),
                };
                let code = rust_waiter_code(&self.name, id, severity);
                let shape = event["shape"].as_str().unwrap_or_default();
                format!("{shape} {severity:?} {code:?}")
            })
            .collect()
    }
}

/// Every `waiters/GROUP/NAME.smithy` fixture, in order.
fn waiter_fixtures() -> Vec<WaiterFixture> {
    let mut found = Vec::new();
    for group in ["valid", "invalid", "differences"] {
        let mut paths: Vec<_> = std::fs::read_dir(fixtures().join("waiters").join(group))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        paths.sort();
        for path in paths {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let name = format!("{group}/{stem}");
            let expected = fixtures().join(format!("expected/waiters/{name}.json"));
            let expected = std::fs::read_to_string(&expected)
                .unwrap_or_else(|e| panic!("{name}: regenerate expected output: {e}"));
            found.push(WaiterFixture {
                name,
                path,
                expected: serde_json::from_str(&expected).unwrap(),
            });
        }
    }
    found
}

/// The Rust diagnostic code corresponding to a Java validation event ID. Unknown IDs fail
/// the test so that regenerated fixtures cannot silently lose their reason check.
fn rust_waiter_code(fixture: &str, java_event: &str, severity: Severity) -> DiagnosticCode {
    match java_event.split('.').next().unwrap() {
        // Java warns about unknown structure members while validating the trait value.
        "TraitValue"
            if java_event.starts_with("TraitValue.UnknownMember.")
                && severity == Severity::Warning =>
        {
            DiagnosticCode::UnknownProperty
        }
        // Creating the trait (`Model`), validating its value and target, and the waiter
        // validators.
        "Model"
        | "TraitValue"
        | "TraitTarget"
        | "WaitableTrait"
        | "WaitableTraitJmespathProblem"
        | "WaitableTraitInvalidErrorType"
        | "UniqueWaiterNames" => DiagnosticCode::InvalidTrait,
        _ => panic!("{fixture}: unmapped Java event `{java_event}`; add it to `rust_waiter_code`"),
    }
}

/// The diagnostics of assembling `path` with the waiter validator, each described by its
/// shape, severity, and code. Notes are left out.
fn waiter_diagnostics(path: &Path) -> BTreeSet<String> {
    Model::assembler()
        .add_idl_file(path)
        .add_validator(waiters::validator())
        .assemble()
        .diagnostics()
        .iter()
        .filter(|d| d.severity() != Severity::Note)
        .map(|d| {
            let shape = d.shape_id().map(ToString::to_string).unwrap_or_default();
            format!("{shape} {:?} {:?}", d.severity(), d.code())
        })
        .collect()
}

/// A decoded trait in the form `RegenerateWaiters.java` records Java's.
fn normalize_waiters(waitable: &Waitable) -> serde_json::Value {
    use serde_json::json;

    let path = |matcher: &PathMatcher| {
        let comparator = match matcher.comparator() {
            PathComparator::StringEquals => "stringEquals",
            PathComparator::BooleanEquals => "booleanEquals",
            PathComparator::AllStringEquals => "allStringEquals",
            PathComparator::AnyStringEquals => "anyStringEquals",
            other => panic!("unexpected comparator {other:?}"),
        };
        json!({"path": matcher.path(), "expected": matcher.expected(), "comparator": comparator})
    };
    let waiters: Vec<_> = waitable
        .waiters()
        .map(|(name, waiter)| {
            let acceptors: Vec<_> = waiter
                .acceptors()
                .iter()
                .map(|acceptor| {
                    let state = match acceptor.state() {
                        AcceptorState::Success => "success",
                        AcceptorState::Failure => "failure",
                        AcceptorState::Retry => "retry",
                        other => panic!("unexpected state {other:?}"),
                    };
                    let matcher = match acceptor.matcher().kind() {
                        MatcherKind::Output(matcher) => json!({"output": path(matcher)}),
                        MatcherKind::InputOutput(matcher) => json!({"inputOutput": path(matcher)}),
                        MatcherKind::Success(success) => json!({"success": success}),
                        MatcherKind::ErrorType(error) => json!({"errorType": error}),
                        other => panic!("unexpected matcher {other:?}"),
                    };
                    json!({"state": state, "matcher": matcher, "message": acceptor.message()})
                })
                .collect();
            json!({
                "name": name,
                "documentation": waiter.documentation(),
                "acceptors": acceptors,
                "minDelay": waiter.min_delay(),
                "maxDelay": waiter.max_delay(),
                "deprecated": waiter.is_deprecated(),
                "tags": waiter.tags(),
            })
        })
        .collect();
    waiters.into()
}

/// Checks that every operation's trait decodes to the waiters Java's typed `WaitableTrait`
/// does. Rust may reject a value Java decodes only if Java rejects the model.
fn check_waiters_match_java(fixture: &WaiterFixture) {
    let name = &fixture.name;
    // The fixtures are only invalid in ways the waiter validator checks.
    let model = Model::from_idl_file(&fixture.path)
        .unwrap_or_else(|e| panic!("{name}: {:?}", e.diagnostics()));
    let java = fixture.expected["waiters"]
        .as_object()
        .unwrap_or_else(|| panic!("{name}: Java built no model"));
    let operations: BTreeSet<String> = model.operations().map(|o| o.id().to_string()).collect();
    assert_eq!(
        java.keys().cloned().collect::<BTreeSet<_>>(),
        operations,
        "{name}: operations"
    );
    for operation in model.operations() {
        let id = operation.id().to_string();
        let java = &java[&id];
        match operation.as_shape().get_trait_as::<Waitable>() {
            Ok(Some(waitable)) if !java.is_null() => {
                assert_eq!(&normalize_waiters(&waitable), java, "{name}: {id}")
            }
            Ok(None) if java.is_null() => {}
            // Rust rejects the value while decoding, which the diagnostics also report.
            Err(_) if !fixture.accepted() => {}
            actual => panic!("{name}: {id}: Java decodes {java}, Rust {actual:?}"),
        }
    }
}

#[test]
fn every_waiter_fixture_has_an_outcome() {
    let recorded: BTreeSet<String> = ["valid", "invalid", "differences"]
        .into_iter()
        .flat_map(|group| {
            std::fs::read_dir(fixtures().join("expected/waiters").join(group))
                .unwrap()
                .map(move |entry| {
                    let path = entry.unwrap().path();
                    format!("{group}/{}", path.file_stem().unwrap().to_string_lossy())
                })
        })
        .collect();
    let names: BTreeSet<String> = waiter_fixtures().into_iter().map(|f| f.name).collect();
    assert_eq!(recorded, names, "regenerate expected/ (see README.md)");

    let differences: BTreeSet<String> = WAITER_DIFFERENCES
        .iter()
        .map(|(name, _)| format!("differences/{name}"))
        .collect();
    let on_disk: BTreeSet<String> = names
        .into_iter()
        .filter(|name| name.starts_with("differences/"))
        .collect();
    assert_eq!(
        differences, on_disk,
        "list every difference in WAITER_DIFFERENCES"
    );
}

/// Agreement with Java's `smithy-waiters` on every `valid/` and `invalid/` waiter fixture:
/// both accept or reject it, report errors and warnings of corresponding codes on the same
/// shapes, and decode the same waiters. The number and wording of diagnostics are not
/// compared.
#[test]
fn waiters_match_java() {
    let mut checked = 0;
    for fixture in waiter_fixtures() {
        let name = &fixture.name;
        if name.starts_with("differences/") {
            continue;
        }
        assert_eq!(
            name.starts_with("valid/"),
            fixture.accepted(),
            "{name}: fixture is in the wrong group"
        );
        assert_eq!(
            waiter_diagnostics(&fixture.path),
            fixture.java_diagnostics(),
            "{name}: Rust diagnostics (left) and Java events (right)"
        );
        check_waiters_match_java(&fixture);
        checked += 1;
    }
    assert!(checked > 0, "no waiter fixtures");
}

/// Every `differences/` waiter fixture disagrees with Java as `WAITER_DIFFERENCES` records,
/// so that a change on either side is noticed. The waiters Java decodes still match.
#[test]
fn waiter_differences_from_java() {
    for fixture in waiter_fixtures() {
        let Some(stem) = fixture.name.strip_prefix("differences/") else {
            continue;
        };
        let name = &fixture.name;
        let (_, expected) = WAITER_DIFFERENCES
            .iter()
            .find(|(difference, _)| *difference == stem)
            .unwrap_or_else(|| panic!("{name}: add the fixture to WAITER_DIFFERENCES"));
        let expected: BTreeSet<String> = expected.iter().map(|d| d.to_string()).collect();
        let actual = waiter_diagnostics(&fixture.path);
        assert_eq!(actual, expected, "{name}: Rust diagnostics");
        assert_ne!(
            actual,
            fixture.java_diagnostics(),
            "{name}: Java now agrees; move the fixture to valid/ or invalid/"
        );
        check_waiters_match_java(&fixture);
    }
}
