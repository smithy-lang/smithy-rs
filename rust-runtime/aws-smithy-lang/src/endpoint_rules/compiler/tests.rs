/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Compares each compiler stage with Smithy Java's, as recorded in `tests/fixtures/endpoint_bdd`
//! (see its README).

use super::builder::Diagram;
use super::cfg::Cfg;
use super::{check, cost_optimization, endpoint_bdd, finish, s3, sift, ssa, unoptimized};
use crate::endpoint_rules::codec::decode_condition;
use crate::endpoint_rules::language::{Rule, RuleKind};
use crate::endpoint_rules::{EndpointBdd, EndpointRuleSet};
use crate::traits::TraitCodec;
use crate::{Model, Node, NodeObject, NodeValue, ShapeId};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Loads a JSON file as an order-preserving [`Node`].
fn load(path: &Path) -> Node {
    let text = std::fs::read_to_string(path).unwrap();
    let wrapped = format!("{{\"smithy\": \"2.0\", \"metadata\": {{\"fixture\": {text}}}}}");
    Model::from_json_str(&path.display().to_string(), &wrapped)
        .unwrap()
        .metadata_value("fixture")
        .unwrap()
        .clone()
}

struct Fixture {
    name: String,
    value: Node,
}

impl Fixture {
    fn get(&self, name: &str) -> Option<&Node> {
        self.value.as_object().unwrap().get(name)
    }

    /// The rule set Java compiled: recorded, or read from the SDK model it came from.
    fn rule_set(&self) -> EndpointRuleSet {
        if let Some(rule_set) = self.get("ruleSet") {
            return EndpointRuleSet::from_node(rule_set).unwrap();
        }
        let source = self.get("source").unwrap().as_str().unwrap();
        let (path, service) = source
            .strip_prefix("aws/sdk/aws-models/")
            .and_then(|s| s.split_once('#'))
            .unwrap();
        let model =
            Model::from_json_file(root().join("../../aws/sdk/aws-models").join(path)).unwrap();
        model
            .expect_shape(&ShapeId::new(service).unwrap())
            .unwrap()
            .get_trait_as::<EndpointRuleSet>()
            .unwrap()
            .unwrap()
    }

    fn member(&self, name: &str) -> &Node {
        self.get(name)
            .unwrap_or_else(|| panic!("{}: missing `{name}`", self.name))
    }

    /// Java's BDD at `stage`, rebuilt from the fixture's compact form.
    fn stage(&self, stage: &str) -> EndpointBdd {
        let compiled = self.member("compiled");
        let field = |node: &Node, name: &str| node.as_object().unwrap().get(name).unwrap().clone();
        let recorded = self.member(stage);
        let cfg_conditions = self.member("cfgConditions").as_array().unwrap();
        let conditions: Vec<Node> = field(recorded, "order")
            .as_array()
            .unwrap()
            .iter()
            .map(|index| {
                let NodeValue::Number(index) = index.value() else {
                    panic!("{}: condition order must be indexes", self.name)
                };
                cfg_conditions[index.as_u64().unwrap() as usize].clone()
            })
            .collect();
        let mut bdd = NodeObject::new();
        for name in ["version", "parameters"] {
            bdd.insert(name, field(compiled, name));
        }
        bdd.insert("conditions", conditions);
        bdd.insert("results", field(compiled, "results"));
        for name in ["root", "nodeCount", "nodes"] {
            bdd.insert(name, field(recorded, name));
        }
        EndpointBdd::from_node(&bdd.into()).unwrap()
    }

    /// Java's BDD at `stage` as a [`Diagram`] over `cfg`, which must be the CFG of this
    /// fixture's rule set, with the results `cfg_results` (results do not depend on order).
    fn diagram(&self, stage: &str, cfg: &Cfg, results: &[usize]) -> Diagram {
        let recorded = self.member(stage).as_object().unwrap();
        let number = |node: &Node| match node.value() {
            NodeValue::Number(n) => n.as_i64().unwrap(),
            _ => panic!("{}: expected a number", self.name),
        };
        let cfg_order = cfg.condition_order();
        let conditions = recorded
            .get("order")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|i| cfg_order[number(i) as usize])
            .collect();
        let bytes = base64_simd::STANDARD
            .decode_to_vec(recorded.get("nodes").unwrap().as_str().unwrap())
            .unwrap();
        let words: Vec<i32> = bytes
            .chunks(4)
            .map(|w| i32::from_be_bytes(w.try_into().unwrap()))
            .collect();
        Diagram {
            root: number(recorded.get("root").unwrap()) as i32,
            // Skip the sentinel.
            nodes: words[3..].chunks(3).map(|n| [n[0], n[1], n[2]]).collect(),
            conditions,
            results: results.to_vec(),
        }
    }

    /// Java's SSA rule set, recorded when it differs from the input.
    fn ssa(&self) -> EndpointRuleSet {
        match self.get("ssa") {
            Some(ssa) => EndpointRuleSet::from_node(ssa).unwrap(),
            None => self.rule_set(),
        }
    }
}

fn fixtures() -> Vec<Fixture> {
    let dir = root().join("tests/fixtures/endpoint_bdd/expected");
    let manifest = load(&dir.join("manifest.json"));
    let names = manifest.as_object().unwrap().get("fixtures").unwrap();
    let fixtures: Vec<_> = names
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, _)| Fixture {
            name: name.to_owned(),
            value: load(&dir.join(format!("{name}.json"))),
        })
        .collect();
    assert!(fixtures.len() >= 96, "{} fixtures", fixtures.len());
    fixtures
}

#[track_caller]
fn first_difference<T: PartialEq + Debug>(context: &str, what: &str, actual: &[T], expected: &[T]) {
    if let Some(i) = actual.iter().zip(expected).position(|(a, e)| a != e) {
        panic!(
            "{context}: {what} {i} differs\n  actual:   {:?}\n  expected: {:?}",
            actual[i], expected[i]
        );
    }
    assert_eq!(actual.len(), expected.len(), "{context}: {what} count");
}

/// Asserts that two rule trees are equal, reporting the first differing rule.
#[track_caller]
fn assert_same_rules(context: &str, actual: &[Rule], expected: &[Rule]) {
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        let context = format!("{context}/{i}");
        first_difference(&context, "condition", &a.conditions, &e.conditions);
        match (&a.kind, &e.kind) {
            (RuleKind::Tree(a), RuleKind::Tree(e)) => assert_same_rules(&context, a, e),
            (a, e) => assert_eq!(a, e, "{context}: result"),
        }
    }
    assert_eq!(actual.len(), expected.len(), "{context}: rule count");
}

#[test]
fn ssa_matches_java() {
    for fixture in fixtures() {
        let rule_set = fixture.rule_set();
        check(&rule_set).unwrap();
        let actual = ssa::transform(&rule_set).unwrap_or_else(|e| panic!("{}: {e}", fixture.name));
        let expected = fixture.ssa();
        assert_same_rules(&fixture.name, &actual.rules, &expected.rules);
        assert_eq!(actual, expected, "{}", fixture.name);
    }
}

#[test]
fn cfg_conditions_match_java() {
    for fixture in fixtures() {
        let cfg = Cfg::new(&fixture.ssa());
        let actual: Vec<_> = cfg
            .condition_order()
            .into_iter()
            .map(|i| cfg.canonical[i].clone())
            .collect();
        let expected: Vec<_> = fixture
            .get("cfgConditions")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|c| decode_condition(c).unwrap())
            .collect();
        first_difference(&fixture.name, "CFG condition", &actual, &expected);
    }
}

fn rule_set(rules: &str) -> EndpointRuleSet {
    let text = format!(
        r#"{{"version": "1.1", "parameters": {{
            "R": {{"type": "string", "required": true, "default": "r", "documentation": "d"}}
        }}, "rules": {rules}}}"#
    );
    let node = Model::from_json_str(
        "test",
        &format!(r#"{{"smithy": "2.0", "metadata": {{"r": {text}}}}}"#),
    )
    .unwrap()
    .metadata_value("r")
    .unwrap()
    .clone();
    EndpointRuleSet::from_node(&node).unwrap()
}

/// Java inlines `c = substring(R, 0, 3)` into `not(stringEquals(c, "x"))`, which then holds
/// when `R` is too short and the assignment would have failed.
#[test]
fn coalescing_keeps_assignments_that_can_fail() {
    let substring = r#"{"fn": "substring", "argv": [{"ref": "R"}, 0, 3, false], "assign": "c"}"#;
    let endpoint = r#""endpoint": {"url": "https://a.example.com"}"#;
    let not_equals =
        r#"{"fn": "not", "argv": [{"fn": "stringEquals", "argv": [{"ref": "c"}, "x"]}]}"#;
    let input = rule_set(&format!(
        r#"[{{"type": "endpoint", "conditions": [{substring}, {not_equals}], {endpoint}}}]"#
    ));
    assert_eq!(ssa::transform(&input).unwrap(), input);

    // Inlining into `stringEquals(c, "x")` is sound: it fails for "".
    let equals = r#"{"fn": "stringEquals", "argv": [{"ref": "c"}, "x"]}"#;
    let input = rule_set(&format!(
        r#"[{{"type": "endpoint", "conditions": [{substring}, {equals}], {endpoint}}}]"#
    ));
    let coalesced = r#"{"fn": "stringEquals", "argv": [
        {"fn": "coalesce", "argv": [{"fn": "substring", "argv": [{"ref": "R"}, 0, 3, false]}, ""]}, "x"
    ]}"#;
    let expected = rule_set(&format!(
        r#"[{{"type": "endpoint", "conditions": [{coalesced}], {endpoint}}}]"#
    ));
    assert_eq!(ssa::transform(&input).unwrap(), expected);
}

/// Asserts that `actual` is the BDD Java produced, reporting the first difference.
#[track_caller]
fn assert_same_bdd(context: &str, actual: &EndpointBdd, expected: &EndpointBdd) {
    assert_eq!(actual.version(), expected.version(), "{context}: version");
    assert_eq!(
        actual.parameters(),
        expected.parameters(),
        "{context}: parameters"
    );
    first_difference(
        context,
        "condition",
        actual.conditions(),
        expected.conditions(),
    );
    first_difference(context, "result", actual.results(), expected.results());
    let (a, e) = (actual.bdd(), expected.bdd());
    assert_eq!(a.root(), e.root(), "{context}: root");
    first_difference(context, "node", a.nodes(), e.nodes());
    assert_eq!(actual, expected, "{context}");
}

#[test]
fn unoptimized_bdd_matches_java() {
    for fixture in fixtures() {
        let rule_set = fixture.rule_set();
        let (ssa, cfg, diagram) = unoptimized(&rule_set).unwrap();
        let actual = endpoint_bdd(&ssa, &cfg, &diagram).unwrap();
        assert_same_bdd(
            &format!("{} unoptimized", fixture.name),
            &actual,
            &fixture.stage("unoptimized"),
        );
    }
}

#[test]
fn cost_optimized_bdd_matches_java() {
    for fixture in fixtures() {
        let (ssa, cfg, unoptimized) = unoptimized(&fixture.rule_set()).unwrap();
        let sifted = fixture.diagram("sifted", &cfg, &unoptimized.results);
        let java = endpoint_bdd(&ssa, &cfg, &sifted).unwrap();
        assert_same_bdd(&fixture.name, &java, &fixture.stage("sifted"));
        let optimized = cost_optimization::optimize(&cfg, sifted).unwrap();
        let actual = endpoint_bdd(&ssa, &cfg, &optimized).unwrap();
        assert_same_bdd(
            &format!("{} cost-optimized", fixture.name),
            &actual,
            &fixture.stage("costOptimized"),
        );
    }
}

#[test]
fn reversed_bdd_matches_java() {
    for fixture in fixtures() {
        let (ssa, cfg, unoptimized) = unoptimized(&fixture.rule_set()).unwrap();
        let optimized = fixture.diagram("costOptimized", &cfg, &unoptimized.results);
        let actual = endpoint_bdd(&ssa, &cfg, &finish::reverse(optimized)).unwrap();
        assert_same_bdd(
            &format!("{} reversed", fixture.name),
            &actual,
            &fixture.stage("reversed"),
        );
    }
}

#[test]
fn compiled_bdd_matches_java() {
    for fixture in fixtures() {
        let (ssa, cfg, unoptimized) = unoptimized(&fixture.rule_set()).unwrap();
        let reversed = fixture.diagram("reversed", &cfg, &unoptimized.results);
        let compiled = finish::remove_unreferenced_conditions(&cfg, reversed);
        let actual = endpoint_bdd(&ssa, &cfg, &compiled).unwrap();
        let expected = EndpointBdd::from_node(fixture.member("compiled")).unwrap();
        assert_same_bdd(&format!("{} compiled", fixture.name), &actual, &expected);
    }
}

/// Runs every stage from the rule set, checking each against Java's. Sifting dominates.
#[test]
fn pipeline_matches_java() {
    for fixture in fixtures() {
        let name = &fixture.name;
        let (ssa, cfg, diagram) = unoptimized(&fixture.rule_set()).unwrap();
        let check = |stage: &str, diagram: &Diagram| {
            let actual = endpoint_bdd(&ssa, &cfg, diagram).unwrap();
            assert_same_bdd(&format!("{name} {stage}"), &actual, &fixture.stage(stage));
        };
        let diagram = sift::sift(&cfg, diagram).unwrap();
        check("sifted", &diagram);
        let diagram = cost_optimization::optimize(&cfg, diagram).unwrap();
        check("costOptimized", &diagram);
        let diagram = finish::reverse(diagram);
        check("reversed", &diagram);
        let diagram = finish::remove_unreferenced_conditions(&cfg, diagram);
        let actual = endpoint_bdd(&ssa, &cfg, &diagram).unwrap();
        let expected = EndpointBdd::from_node(fixture.member("compiled")).unwrap();
        assert_same_bdd(&format!("{name} compiled"), &actual, &expected);
        // `compile` runs the same stages; check it where that is quick.
        if diagram.nodes.len() < 200 {
            assert_eq!(
                super::compile(&fixture.rule_set()).unwrap(),
                expected,
                "{name}"
            );
        }
    }
}

/// `S3TreeRewriter` produces Java's rewritten S3 rule set, which `pipeline_matches_java`
/// compiles to the checked-in S3 BDD.
#[test]
fn s3_rewrite_matches_java() {
    let fixtures = fixtures();
    let fixture = |name: &str| fixtures.iter().find(|f| f.name == name).unwrap();
    let original = fixture("aws-s3").rule_set();
    // Not `assert_eq!`, a rule set's debug form is megabytes and would print on failure
    assert!(s3::rewrite(&original) == fixture("aws-s3-rewritten").rule_set());
}

/// From S3's original rule set, `compile_for_aws` produces the checked-in S3 BDD (which
/// `tests/endpoint_bdd_java.rs` checks the `aws-s3-rewritten` fixture records); other
/// services compile as with `compile`.
#[test]
fn compile_for_aws_matches_checked_in_bdds() {
    let fixtures = fixtures();
    let fixture = |name: &str| fixtures.iter().find(|f| f.name == name).unwrap();
    let checked_in = |name: &str| EndpointBdd::from_node(fixture(name).member("compiled")).unwrap();
    let s3 = super::compile_for_aws(s3::SERVICE, &fixture("aws-s3").rule_set()).unwrap();
    assert_eq!(s3.bdd().wire_node_count(), 553);
    assert_same_bdd("aws-s3", &s3, &checked_in("aws-s3-rewritten"));
    let other = super::compile_for_aws(
        "com.amazonaws.sqs#AmazonSQS",
        &fixture("aws-sqs").rule_set(),
    );
    assert_eq!(other.unwrap(), checked_in("aws-sqs"));
}
