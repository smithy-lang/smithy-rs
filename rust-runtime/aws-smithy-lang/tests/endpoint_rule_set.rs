/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Decodes, inspects, validates, and round-trips `smithy.rules#endpointRuleSet` traits.

use aws_smithy_lang::diagnostic::{Diagnostic, Severity};
use aws_smithy_lang::endpoint_rules::{
    self, EndpointBdd, EndpointRuleSet, EndpointRulesLimits, ExpressionKind, RuleKind,
    RulesVersion, ENDPOINT_RULE_SET,
};
use aws_smithy_lang::traits::{TraitCodec, TraitDecodeError};
use aws_smithy_lang::{Model, Node, ShapeId};
use serde_json::{json, Value};
use std::path::PathBuf;

const TRAIT: &str = "/shapes/ex#S/traits/smithy.rules#endpointRuleSet";

/// Every corpus model with a rule set.
fn corpus() -> Vec<(PathBuf, Model)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../aws/sdk/aws-models");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            if !text.contains("smithy.rules#endpointRuleSet") {
                return None;
            }
            let model = Model::from_json_str(&path.display().to_string(), &text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            Some((path, model))
        })
        .collect()
}

#[test]
fn corpus_decodes_round_trips_and_validates() {
    let models = corpus();
    assert!(models.len() >= 20, "found {} rule-set models", models.len());
    let (mut traits, mut rules) = (0, 0);
    for (path, model) in &models {
        for shape in model.shapes_with_trait(&ENDPOINT_RULE_SET) {
            traits += 1;
            let original = shape.get_trait(&ENDPOINT_RULE_SET).unwrap();
            let rule_set = shape
                .get_trait_as::<EndpointRuleSet>()
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .unwrap();
            assert_eq!(&rule_set.to_node(), original, "{}", path.display());
            assert_eq!(
                EndpointRuleSet::from_node(&rule_set.to_node()).unwrap(),
                rule_set
            );
            assert_eq!(rule_set.validate(), [], "{}", path.display());
            rules += rule_set.iter_rules().count();

            // The rule set and the BDD compiled from it share the parameter language.
            if let Some(bdd) = shape.get_trait_as::<EndpointBdd>().unwrap() {
                assert_eq!(bdd.parameters(), rule_set.parameters());
            }
        }
        let validated = Model::assembler()
            .add_model(model)
            .add_validator(endpoint_rules::validator())
            .assemble();
        let diagnostics: Vec<_> = validated
            .diagnostics()
            .iter()
            .filter(|d| d.validator() == Some("EndpointRules"))
            .map(|d| d.to_string())
            .collect();
        assert!(
            diagnostics.is_empty(),
            "{}: {diagnostics:#?}",
            path.display()
        );
    }
    assert!(rules > 1_000, "walked {rules} rules in {traits} rule sets");
}

/// A rule set exercising every rule kind, nesting, and scoping.
fn rule_set() -> Value {
    json!({
        "version": "1.0",
        "parameters": {
            "Region": {"type": "string", "builtIn": "AWS::Region", "required": false, "documentation": "d"}
        },
        "rules": [
            {
                "type": "tree",
                "documentation": "has region",
                "conditions": [
                    {"fn": "isSet", "argv": [{"ref": "Region"}]},
                    {"fn": "aws.partition", "argv": [{"ref": "Region"}], "assign": "Partition"}
                ],
                "rules": [
                    {
                        "type": "endpoint",
                        "conditions": [
                            {"fn": "getAttr", "argv": [{"ref": "Partition"}, "name"], "assign": "Name"}
                        ],
                        "endpoint": {
                            "url": "https://{Region}.{Partition#dnsSuffix}",
                            "properties": {},
                            "headers": {}
                        }
                    },
                    {
                        "type": "error",
                        "conditions": [{"fn": "stringEquals", "argv": [{"ref": "Region"}, "x"], "assign": "Name"}],
                        "error": "bad {Region}"
                    }
                ]
            },
            {"type": "error", "conditions": [], "error": "Missing Region"}
        ]
    })
}

fn trait_node(value: &Value) -> Node {
    let model = model(value.clone());
    let model = Model::from_json_str("model.json", &model.to_string()).unwrap();
    model
        .expect_shape(&ShapeId::from_static("ex#S"))
        .unwrap()
        .get_trait(&ENDPOINT_RULE_SET)
        .unwrap()
        .clone()
}

fn model(rule_set: Value) -> Value {
    json!({
        "smithy": "2.0",
        "shapes": {
            "ex#S": {"type": "service", "traits": {"smithy.rules#endpointRuleSet": rule_set}}
        }
    })
}

fn decode_with(
    value: &Value,
    limits: &EndpointRulesLimits,
) -> Result<EndpointRuleSet, TraitDecodeError> {
    EndpointRuleSet::from_node_with_limits(&trait_node(value), limits)
}

fn expect_error(mutate: impl FnOnce(&mut Value), message: &str, pointer: &str) {
    expect_error_with(mutate, &EndpointRulesLimits::default(), message, pointer)
}

fn expect_error_with(
    mutate: impl FnOnce(&mut Value),
    limits: &EndpointRulesLimits,
    message: &str,
    pointer: &str,
) {
    let mut value = rule_set();
    mutate(&mut value);
    let err = decode_with(&value, limits).expect_err(message);
    assert!(err.message().contains(message), "{:?}", err.message());
    assert_eq!(
        err.source_location().pointer(),
        Some(&*format!("{TRAIT}{pointer}")),
        "{}",
        err.message()
    );
}

#[test]
fn decodes_rule_trees() {
    let rules = EndpointRuleSet::from_node(&trait_node(&rule_set())).unwrap();
    assert_eq!(rules.version(), RulesVersion::V1_0);
    assert_eq!(rules.rules().len(), 2);

    let tree = &rules.rules()[0];
    assert_eq!(tree.documentation(), Some("has region"));
    assert_eq!(tree.conditions()[1].assign(), Some("Partition"));
    assert_eq!(
        tree.source_location().pointer(),
        Some(&*format!("{TRAIT}/rules/0"))
    );
    let RuleKind::Tree(children) = tree.kind() else {
        panic!("expected a tree rule");
    };
    assert!(matches!(children[0].kind(), RuleKind::Endpoint(_)));
    let RuleKind::Error(message) = children[1].kind() else {
        panic!("expected an error rule");
    };
    assert!(matches!(message.kind(), ExpressionKind::Literal(_)));

    // Document order: a tree precedes its children; siblings keep their order.
    let order: Vec<_> = rules
        .iter_rules()
        .map(|rule| {
            rule.source_location()
                .pointer()
                .unwrap()
                .strip_prefix(TRAIT)
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        order,
        [
            "/rules/0",
            "/rules/0/rules/0",
            "/rules/0/rules/1",
            "/rules/1"
        ]
    );

    // Missing `conditions` is accepted, as in the reference implementation, and re-encoded.
    let mut value = rule_set();
    value["rules"][1]
        .as_object_mut()
        .unwrap()
        .remove("conditions");
    let decoded = decode_with(&value, &EndpointRulesLimits::default()).unwrap();
    assert!(decoded.rules()[1].conditions().is_empty());
    assert_eq!(decoded.to_node(), trait_node(&rule_set()));

    // Unknown root members are tolerated and not re-encoded.
    let mut value = rule_set();
    value["extension"] = json!(true);
    assert_eq!(
        decode_with(&value, &EndpointRulesLimits::default())
            .unwrap()
            .to_node(),
        trait_node(&rule_set())
    );
}

#[test]
fn rejects_malformed_rules() {
    expect_error(
        |v| {
            v.as_object_mut()
                .unwrap()
                .remove("rules")
                .map(drop)
                .unwrap()
        },
        "missing required member `rules`",
        "",
    );
    expect_error(
        |v| v["rules"] = json!({}),
        "expected a `rules` array",
        "/rules",
    );
    expect_error(
        |v| v["rules"][1]["type"] = "other".into(),
        "rule `type` must be `endpoint`, `error`, or `tree`",
        "/rules/1/type",
    );
    expect_error(
        |v| {
            v["rules"][1].as_object_mut().unwrap().remove("type");
        },
        "rule is missing `type`",
        "/rules/1",
    );
    expect_error(
        |v| {
            v["rules"][0].as_object_mut().unwrap().remove("rules");
        },
        "rule is missing `rules`",
        "/rules/0",
    );
    expect_error(
        |v| v["rules"][1]["rules"] = json!([]),
        "a `error` rule must not have `rules`",
        "/rules/1/rules",
    );
    expect_error(
        |v| v["rules"][1]["extra"] = json!(1),
        "extra",
        "/rules/1/extra",
    );
    expect_error(
        |v| v["rules"][0]["rules"][0]["conditions"][0]["argv"][1] = "a[".into(),
        "invalid attribute path `a[`",
        "/rules/0/rules/0/conditions/0/argv/1",
    );
}

#[test]
fn negative_indexes_require_version_1_1() {
    let set_path = |v: &mut Value, path: &str| {
        v["rules"][0]["rules"][0]["conditions"][0]["argv"][1] = path.into();
    };
    expect_error(
        |v| set_path(v, "name[-1]"),
        "negative attribute path indexes require rules engine version 1.1 or later, but the trait declares version 1.0",
        "/rules/0/rules/0/conditions/0/argv/1",
    );
    // Template references are paths too.
    expect_error(
        |v| v["rules"][1]["error"] = "{Region#a[-2]}".into(),
        "require rules engine version 1.1",
        "/rules/1/error",
    );
    let mut value = rule_set();
    set_path(&mut value, "name[-1]");
    value["version"] = "1.1".into();
    decode_with(&value, &EndpointRulesLimits::default()).unwrap();
}

#[test]
fn enforces_rule_limits() {
    let limits = EndpointRulesLimits::default().with_max_rule_depth(1);
    expect_error_with(
        |_| {},
        &limits,
        "rule nesting exceeds the limit of 1",
        "/rules/0/rules",
    );
    let limits = EndpointRulesLimits::default().with_max_rules(3);
    expect_error_with(|_| {}, &limits, "rules", "/rules/1");
    let limits = EndpointRulesLimits::default().with_max_conditions(3);
    expect_error_with(|_| {}, &limits, "conditions", "/rules/0/rules/1/conditions");

    // Deep nesting (the model loader's own depth limit admits about 48 tree levels): decodes,
    // walks, validates, and re-encodes, and a lower rule-depth limit is enforced.
    let mut rule = json!({"type": "error", "conditions": [], "error": "x"});
    for _ in 0..40 {
        rule = json!({"type": "tree", "conditions": [], "rules": [rule]});
    }
    let mut value = rule_set();
    value["rules"] = json!([rule]);
    let rules = decode_with(&value, &EndpointRulesLimits::default()).unwrap();
    assert_eq!(rules.iter_rules().count(), 41);
    assert_eq!(rules.validate(), []);
    assert_eq!(rules.to_node(), trait_node(&value));
    let limits = EndpointRulesLimits::default().with_max_rule_depth(40);
    let err = decode_with(&value, &limits).unwrap_err();
    assert!(
        err.message()
            .contains("rule nesting exceeds the limit of 40"),
        "{}",
        err.message()
    );
}

fn validate(value: Value) -> Vec<Diagnostic> {
    EndpointRuleSet::from_node(&trait_node(&value))
        .unwrap()
        .validate()
}

fn expect_one(mutate: impl FnOnce(&mut Value), message: &str, pointer: &str) -> Diagnostic {
    let mut value = rule_set();
    mutate(&mut value);
    let diagnostics = validate(value);
    let [diagnostic] = &diagnostics[..] else {
        panic!("expected one diagnostic, found {diagnostics:#?}");
    };
    assert_eq!(diagnostic.severity(), Severity::Error);
    assert!(diagnostic.message().contains(message), "{diagnostic}");
    assert_eq!(
        diagnostic.source_location().and_then(|l| l.pointer()),
        Some(&*format!("{TRAIT}{pointer}")),
        "{diagnostic}"
    );
    diagnostic.clone()
}

#[test]
fn validates_scopes() {
    // Sibling rules may reuse a name (both children assign `Name`).
    assert_eq!(validate(rule_set()), []);

    // A nested rule may not reassign an ancestor's name.
    let diagnostic = expect_one(
        |v| v["rules"][0]["rules"][0]["conditions"][0]["assign"] = "Partition".into(),
        "condition assigns `Partition`, which is already assigned in scope",
        "/rules/0/rules/0/conditions/0",
    );
    assert_eq!(
        diagnostic.related()[0].pointer(),
        Some(&*format!("{TRAIT}/rules/0/conditions/1"))
    );
    expect_one(
        |v| v["rules"][0]["conditions"][0]["assign"] = "Region".into(),
        "condition assigns `Region`, which is a parameter name",
        "/rules/0/conditions/0",
    );

    // An assignment is not visible to a sibling rule or outside its tree.
    expect_one(
        |v| {
            // Rule 1 no longer assigns `Name`; rule 0's assignment is not visible to it.
            v["rules"][0]["rules"][1]["conditions"][0]
                .as_object_mut()
                .unwrap()
                .remove("assign");
            v["rules"][0]["rules"][1]["error"] = json!({"ref": "Name"});
        },
        "rule references `Name`, which is not a parameter or an assignment in scope",
        "/rules/0/rules/1/error",
    );
    expect_one(
        |v| v["rules"][1]["error"] = "{Partition#name}".into(),
        "rule references `Partition`",
        "/rules/1/error",
    );
    // Nor to an earlier condition of the same rule.
    expect_one(
        |v| v["rules"][0]["conditions"][0]["argv"][0] = json!({"ref": "Partition"}),
        "condition references `Partition`, which is not a parameter or an assignment in scope",
        "/rules/0/conditions/0/argv/0",
    );
    expect_one(
        |v| v["rules"][0]["rules"][0]["endpoint"]["url"] = "ftp://{Region}".into(),
        "must start with `http://` or `https://`",
        "/rules/0/rules/0/endpoint/url",
    );
}

#[test]
fn model_validator_checks_rule_sets() {
    let report = |value: Value| -> Vec<Diagnostic> {
        Model::assembler()
            .add_json_str("model.json", model(value).to_string())
            .add_validator(endpoint_rules::validator())
            .assemble()
            .diagnostics()
            .iter()
            .filter(|d| d.validator() == Some("EndpointRules"))
            .cloned()
            .collect()
    };
    assert_eq!(report(rule_set()), []);

    let mut value = rule_set();
    value["extension"] = json!(1);
    let diagnostics = report(value);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].severity(), Severity::Warning);
    assert!(diagnostics[0]
        .message()
        .contains("unknown endpointRuleSet member `extension`"));

    let mut value = rule_set();
    value["rules"][1]["type"] = "other".into();
    let diagnostics = report(value);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0]
        .message()
        .contains("invalid `smithy.rules#endpointRuleSet` trait value"));

    // Bindings are checked against the rule set when there is no BDD.
    let mut value = rule_set();
    value["parameters"]["Bucket"] = json!({"type": "string", "documentation": "d"});
    let diagnostics = report(value);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0]
        .message()
        .contains("endpoint parameter `Bucket` is neither built-in nor bound"));
}
