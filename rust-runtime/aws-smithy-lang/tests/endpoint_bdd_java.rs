/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy Java `CompileBdd` parity fixtures (`tests/fixtures/endpoint_bdd`).
//!
//! Each fixture records every stage of the pinned Java compiler for one rule set. These tests
//! check that the fixtures cover and still match the checked-in models, and that this crate
//! decodes and re-encodes every Java stage exactly. A Rust compiler stage is held to the same
//! standard by [`assert_same_bdd`]: it must produce the identical diagram, condition order,
//! and results.
//!
//! Fixtures are loaded as model metadata rather than `serde_json::Value`s, so object member
//! order (which is significant for parameters) and source locations are kept.

use aws_smithy_lang::endpoint_rules::{
    EndpointBdd, EndpointRuleSet, ENDPOINT_BDD, ENDPOINT_RULE_SET,
};
use aws_smithy_lang::traits::TraitCodec;
use aws_smithy_lang::{Model, Node, NodeObject, NodeValue, ShapeId};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const PINNED: &str = "1.73.0";
/// The BDD stages of `CompileBdd.compileBdd` before the last, `compiled`, in order.
const STAGES: [&str; 4] = ["unoptimized", "sifted", "costOptimized", "reversed"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn aws_models() -> PathBuf {
    root().join("../../aws/sdk/aws-models")
}

fn fixtures_dir() -> PathBuf {
    root().join("tests/fixtures/endpoint_bdd/expected")
}

/// Loads a JSON file as a source-located, order-preserving [`Node`].
fn load(path: &Path) -> Node {
    let text = std::fs::read_to_string(path).unwrap();
    let wrapped = format!("{{\"smithy\": \"2.0\", \"metadata\": {{\"fixture\": {text}}}}}");
    Model::from_json_str(&path.display().to_string(), &wrapped)
        .unwrap()
        .metadata_value("fixture")
        .unwrap()
        .clone()
}

#[track_caller]
fn member<'a>(node: &'a Node, name: &str) -> &'a Node {
    node.as_object()
        .and_then(|o| o.get(name))
        .unwrap_or_else(|| panic!("missing `{name}`"))
}

/// A recorded Java compilation.
struct Fixture {
    name: String,
    value: Node,
}

impl Fixture {
    fn get(&self, name: &str) -> Option<&Node> {
        self.value.as_object().unwrap().get(name)
    }

    fn str(&self, name: &str) -> &str {
        member(&self.value, name).as_str().unwrap()
    }

    fn matches_checked_in(&self) -> bool {
        member(&self.value, "matchesCheckedIn").value() == &NodeValue::Bool(true)
    }

    /// The rule set Java compiled: recorded, or read from the model it came from.
    fn rule_set(&self) -> EndpointRuleSet {
        if let Some(rule_set) = self.get("ruleSet") {
            return EndpointRuleSet::from_node(rule_set).unwrap();
        }
        let (model, service) = self.model();
        model
            .expect_shape(&service)
            .unwrap()
            .get_trait_as::<EndpointRuleSet>()
            .unwrap()
            .unwrap()
    }

    /// The rule set after Java's SSA transform (and the rewrites that precede it).
    fn ssa(&self) -> EndpointRuleSet {
        match self.get("ssa") {
            Some(ssa) => EndpointRuleSet::from_node(ssa).unwrap(),
            None => self.rule_set(),
        }
    }

    /// For an `aws/sdk/aws-models` fixture, the model and service it was compiled from.
    fn model(&self) -> (Model, ShapeId) {
        let source = self.str("source");
        let (path, service) = source
            .strip_prefix("aws/sdk/aws-models/")
            .and_then(|s| s.split_once('#'))
            .unwrap_or_else(|| panic!("{}: not an aws-models fixture: {source}", self.name));
        let model = Model::from_json_file(aws_models().join(path)).unwrap();
        (model, ShapeId::new(service).unwrap())
    }

    /// The trait node for a BDD stage, rebuilt from the compact fixture form.
    fn stage_node(&self, stage: &str) -> Node {
        let compiled = member(&self.value, "compiled");
        if stage == "compiled" {
            return compiled.clone();
        }
        let recorded = member(&self.value, stage);
        let cfg_conditions = member(&self.value, "cfgConditions").as_array().unwrap();
        let conditions: Vec<Node> = member(recorded, "order")
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
        bdd.insert("version", member(compiled, "version").clone());
        bdd.insert("parameters", member(compiled, "parameters").clone());
        bdd.insert("conditions", conditions);
        bdd.insert("results", member(compiled, "results").clone());
        for name in ["root", "nodeCount", "nodes"] {
            bdd.insert(name, member(recorded, name).clone());
        }
        bdd.into()
    }

    fn stage(&self, stage: &str) -> EndpointBdd {
        EndpointBdd::from_node(&self.stage_node(stage))
            .unwrap_or_else(|e| panic!("{} {stage}: {e}", self.name))
    }
}

fn fixtures() -> Vec<Fixture> {
    let manifest = load(&fixtures_dir().join("manifest.json"));
    assert_eq!(
        member(&manifest, "smithyRulesEngine").as_str(),
        Some(PINNED)
    );
    member(&manifest, "fixtures")
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, _)| Fixture {
            name: name.to_owned(),
            value: load(&fixtures_dir().join(format!("{name}.json"))),
        })
        .collect()
}

/// Asserts that `actual` is the BDD Java produced, reporting the first difference.
///
/// Compares the version, parameters, condition order, results, root, and every node, so a
/// failure names what diverged rather than printing two whole diagrams.
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

#[track_caller]
fn first_difference<T: PartialEq + std::fmt::Debug>(
    context: &str,
    what: &str,
    actual: &[T],
    expected: &[T],
) {
    if let Some(i) = actual.iter().zip(expected).position(|(a, e)| a != e) {
        panic!(
            "{context}: {what} {i} differs\n  actual:   {:?}\n  expected: {:?}",
            actual[i], expected[i]
        );
    }
    assert_eq!(actual.len(), expected.len(), "{context}: {what} count");
}

/// The fixtures cover every rule set in the SDK models, and still match them.
#[test]
fn fixtures_match_the_checked_in_models() {
    let fixtures = fixtures();
    let mut sources = BTreeSet::new();
    for entry in std::fs::read_dir(aws_models()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        if !text.contains("smithy.rules#endpointRuleSet") {
            continue;
        }
        let model = Model::from_json_str(&path.display().to_string(), &text).unwrap();
        for shape in model.shapes_with_trait(&ENDPOINT_RULE_SET) {
            let file = path.file_name().unwrap().to_str().unwrap();
            sources.insert(format!("aws/sdk/aws-models/{file}#{}", shape.id()));
        }
    }
    let recorded: BTreeSet<_> = fixtures
        .iter()
        .filter(|f| f.str("pipeline") == "CompileBdd" && f.str("source").starts_with("aws/"))
        .map(|f| f.str("source").to_owned())
        .collect();
    assert_eq!(recorded, sources, "regenerate the fixtures (see README.md)");

    // Every SDK BDD is Java output; S3's is compiled from its rewritten rule set.
    let mut checked = 0;
    for fixture in fixtures.iter().filter(|f| f.matches_checked_in()) {
        let (model, service) = fixture.model();
        let shape = model.expect_shape(&service).unwrap();
        assert_eq!(
            shape.get_trait(&ENDPOINT_BDD).unwrap(),
            member(&fixture.value, "compiled"),
            "{}: the checked-in endpointBdd changed; regenerate the fixtures (see README.md)",
            fixture.name
        );
        checked += 1;
    }
    assert_eq!(
        checked,
        sources.len(),
        "every checked-in BDD has a matching fixture"
    );
}

/// Every Java stage decodes under default limits and re-encodes to Java's exact trait node.
#[test]
fn java_stages_decode_and_round_trip() {
    let fixtures = fixtures();
    assert!(fixtures.len() >= 37, "{} fixtures", fixtures.len());
    for fixture in &fixtures {
        let name = &fixture.name;
        let rule_set = fixture.rule_set();
        let ssa = fixture.ssa();
        assert_eq!(rule_set.validate(), [], "{name}: input rule set");
        assert_eq!(ssa.validate(), [], "{name}: SSA rule set");
        assert_eq!(
            ssa.parameters(),
            rule_set.parameters(),
            "{name}: SSA keeps parameters"
        );

        let cfg_conditions = member(&fixture.value, "cfgConditions").as_array().unwrap();
        let compiled = fixture.stage("compiled");
        for stage in STAGES.into_iter().chain(["compiled"]) {
            let context = format!("{name} {stage}");
            let bdd = fixture.stage(stage);
            assert_eq!(
                bdd.to_node(),
                fixture.stage_node(stage),
                "{context}: re-encoding"
            );
            assert_eq!(
                bdd.parameters(),
                rule_set.parameters(),
                "{context}: parameters"
            );
            assert_eq!(bdd.results(), compiled.results(), "{context}: results");
            assert!(bdd.conditions().len() <= cfg_conditions.len(), "{context}");
        }
    }
}

/// The comparison reports the first divergence rather than accepting a near miss.
#[test]
#[should_panic(expected = "aws-sts sifted: node 0 differs")]
fn stage_comparison_reports_differences() {
    let fixture = fixtures()
        .into_iter()
        .find(|f| f.name == "aws-sts")
        .unwrap();
    let sifted = fixture.stage("sifted");
    let mut swapped = fixture.stage_node("sifted").as_object().unwrap().clone();
    let mut bytes = base64_simd::STANDARD
        .decode_to_vec(swapped.get("nodes").unwrap().as_str().unwrap())
        .unwrap();
    // Swap the high and low edges of the first decision node (wire node 1).
    let (high, low) = bytes[16..24].split_at_mut(4);
    high.swap_with_slice(low);
    swapped.insert("nodes", base64_simd::STANDARD.encode_to_string(&bytes));
    let swapped = EndpointBdd::from_node(&swapped.into()).unwrap();
    assert_same_bdd("aws-sts sifted", &swapped, &sifted);
}
