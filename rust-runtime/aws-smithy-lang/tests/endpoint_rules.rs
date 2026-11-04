/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Decodes, inspects, and round-trips `smithy.rules#endpointBdd` traits.

use aws_smithy_lang::endpoint_rules::{
    BddRef, EndpointBdd, EndpointRulesLimits, ExpressionKind, Literal, ParameterType,
    ParameterValue, PathSegment, ResultKind, RulesVersion, TemplateSegment, ENDPOINT_BDD,
};
use aws_smithy_lang::traits::{TraitCodec, TraitDecodeError};
use aws_smithy_lang::{validation, Model, Node, NodeObject, NodeValue, Number, ShapeId};
use std::path::PathBuf;

const R: i32 = 100_000_000;

/// Every corpus model, loaded.
fn corpus() -> Vec<(PathBuf, Model)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../aws/sdk/aws-models");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            if !text.contains("smithy.rules#endpointBdd") {
                return None;
            }
            let model = Model::from_json_str(&path.display().to_string(), &text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            Some((path, model))
        })
        .collect()
}

#[test]
fn corpus_decodes_and_round_trips() {
    let models = corpus();
    assert!(
        models.len() >= 20,
        "expected the checked-in endpointBdd corpus, found {}",
        models.len()
    );
    let mut traits = 0;
    for (path, model) in &models {
        for shape in model.shapes_with_trait(&ENDPOINT_BDD) {
            traits += 1;
            let original = shape.get_trait(&ENDPOINT_BDD).unwrap();
            let rules = shape
                .get_trait_as::<EndpointBdd>()
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .unwrap();

            // The encoded trait equals the original, including the exact node payload.
            let encoded = rules.to_node();
            assert_eq!(&encoded, original, "{}", path.display());
            let decoded = EndpointBdd::from_node(&encoded).unwrap();
            assert_eq!(decoded, rules, "{}", path.display());

            check_invariants(&rules);
        }
    }
    println!(
        "endpointBdd corpus: {} models, {traits} traits",
        models.len()
    );
}

/// Checks the guarantees documented on `EndpointBdd`.
fn check_invariants(rules: &EndpointBdd) {
    let bdd = rules.bdd();
    assert!(rules.version() >= RulesVersion::V1_1);
    assert_eq!(bdd.condition_count(), rules.conditions().len());
    assert_eq!(bdd.result_count(), rules.results().len() + 1);
    let edges = bdd.nodes().iter().flat_map(|n| [n.high(), n.low()]);
    for reference in std::iter::once(bdd.root()).chain(edges) {
        match reference {
            BddRef::Node(node) => {
                let node = bdd.node(node);
                assert!(node.condition() < rules.conditions().len());
            }
            BddRef::Result(result) => {
                assert_eq!(rules.result(result).is_none(), result.is_no_match());
            }
            BddRef::Terminal(_) => {}
        }
    }
    assert!(bdd
        .referenced_results()
        .all(|r| r.index() < bdd.result_count()));
    for condition in rules.conditions() {
        assert!(!condition.function_name().is_empty());
    }
}

#[test]
fn decode_errors_are_source_located() {
    let (_, model) = corpus().into_iter().next().unwrap();
    let id = ENDPOINT_BDD;
    let shape = model.shapes_with_trait(&id).next().unwrap();
    let rules = shape.get_trait_as::<EndpointBdd>().unwrap().unwrap();
    // Every decoded syntax value points back into the loaded model.
    let parameter = rules.parameters().iter().next().unwrap();
    let pointer = parameter.source_location().pointer().unwrap();
    assert!(
        pointer.ends_with(&format!(
            "/traits/smithy.rules#endpointBdd/parameters/{}",
            parameter.name()
        )),
        "{pointer}"
    );
    assert!(parameter.source_location().line().is_some());
    let condition = &rules.conditions()[0];
    assert!(condition
        .source_location()
        .pointer()
        .unwrap()
        .ends_with("/conditions/0"));
    assert!(condition.arguments()[0]
        .source_location()
        .pointer()
        .unwrap()
        .ends_with("/conditions/0/argv/0"));
    assert!(rules.results()[0]
        .source_location()
        .pointer()
        .unwrap()
        .ends_with("/results/0"));
}

/// A minimal valid trait, as JSON, for mutation by tests.
fn minimal() -> serde_json::Value {
    serde_json::json!({
        "version": "1.1",
        "parameters": {
            "Region": {"type": "string", "required": false, "builtIn": "AWS::Region"},
            "UseFIPS": {"type": "boolean", "required": true, "default": false}
        },
        "conditions": [
            {"fn": "isSet", "argv": [{"ref": "Region"}]},
            {"fn": "aws.partition", "argv": [{"ref": "Region"}], "assign": "Partition"}
        ],
        "results": [
            {
                "type": "endpoint",
                "conditions": [],
                "endpoint": {
                    "url": "https://{Region}.{Partition#dnsSuffix}",
                    "properties": {"authSchemes": [{"name": "sigv4", "disableDoubleEncoding": true}]},
                    "headers": {"x-a": ["{Region}", "b"]}
                }
            },
            {"type": "error", "conditions": [], "error": "Invalid Configuration: Missing Region"}
        ],
        "root": 2,
        "nodeCount": 3,
        "nodes": nodes(&[[0, 3, R + 2], [1, R + 1, R]])
    })
}

fn nodes(nodes: &[[i32; 3]]) -> String {
    let mut bytes = Vec::new();
    for value in [-1, 1, -1]
        .into_iter()
        .chain(nodes.iter().flatten().copied())
    {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Loads `value` as the trait of a service and decodes it.
fn decode(value: serde_json::Value) -> Result<EndpointBdd, TraitDecodeError> {
    decode_with(value, &EndpointRulesLimits::default())
}

fn decode_with(
    value: serde_json::Value,
    limits: &EndpointRulesLimits,
) -> Result<EndpointBdd, TraitDecodeError> {
    let node = trait_node(value);
    EndpointBdd::from_node_with_limits(&node, limits)
}

fn trait_node(value: serde_json::Value) -> Node {
    let model = serde_json::json!({
        "smithy": "2.0",
        "shapes": {"ex#S": {"type": "service", "traits": {"smithy.rules#endpointBdd": value}}}
    });
    let model = Model::from_json_str("model.json", &model.to_string()).unwrap();
    model
        .expect_shape(&ShapeId::from_static("ex#S"))
        .unwrap()
        .get_trait(&ENDPOINT_BDD)
        .unwrap()
        .clone()
}

/// Decodes a mutated minimal trait, expecting an error whose message contains `message`
/// and whose pointer ends with `pointer` (relative to the trait).
fn expect_error(mutate: impl FnOnce(&mut serde_json::Value), message: &str, pointer: &str) {
    let mut value = minimal();
    mutate(&mut value);
    let err = decode(value).expect_err(message);
    assert!(
        err.message().contains(message),
        "expected {message:?} in {:?}",
        err.message()
    );
    let actual = err.source_location().pointer().unwrap();
    let expected = format!("/shapes/ex#S/traits/smithy.rules#endpointBdd{pointer}");
    assert_eq!(actual, expected, "{}", err.message());
}

#[test]
fn decodes_the_language() {
    let rules = decode(minimal()).unwrap();
    assert_eq!(rules.version(), RulesVersion::new(1, 1));

    let names: Vec<_> = rules.parameters().iter().map(|p| p.name()).collect();
    assert_eq!(names, ["Region", "UseFIPS"]);
    let region = rules.parameters().get("Region").unwrap();
    assert_eq!(region.kind(), ParameterType::String);
    assert!(!region.required());
    assert_eq!(region.built_in(), Some("AWS::Region"));
    let fips = rules.parameters().get("UseFIPS").unwrap();
    assert_eq!(fips.default(), Some(&ParameterValue::Boolean(false)));

    let partition = &rules.conditions()[1];
    assert_eq!(partition.function_name(), "aws.partition");
    assert_eq!(partition.assign(), Some("Partition"));
    assert!(matches!(
        partition.arguments()[0].kind(),
        ExpressionKind::Reference(name) if name == "Region"
    ));

    let ResultKind::Endpoint(endpoint) = rules.results()[0].kind() else {
        panic!()
    };
    let ExpressionKind::Literal(Literal::String(url)) = endpoint.url().kind() else {
        panic!()
    };
    assert_eq!(
        url.segments(),
        [
            TemplateSegment::Literal("https://".into()),
            TemplateSegment::Reference {
                name: "Region".into(),
                path: Box::new([])
            },
            TemplateSegment::Literal(".".into()),
            TemplateSegment::Reference {
                name: "Partition".into(),
                path: Box::new([PathSegment::Key("dnsSuffix".into())])
            },
        ]
    );
    let headers: Vec<_> = endpoint.headers().map(|(n, v)| (n, v.len())).collect();
    assert_eq!(headers, [("x-a", 2)]);
    let (name, Literal::Tuple(schemes)) = endpoint.properties().next().unwrap() else {
        panic!()
    };
    assert_eq!(name, "authSchemes");
    let Literal::Record(scheme) = &schemes[0] else {
        panic!()
    };
    let field = |key: &str| &scheme.iter().find(|(k, _)| k == key).unwrap().1;
    assert_eq!(field("disableDoubleEncoding"), &Literal::Boolean(true));
    assert!(matches!(field("name"), Literal::String(t) if t.as_static() == Some("sigv4")));
    assert!(matches!(rules.results()[1].kind(), ResultKind::Error(_)));
}

#[test]
fn round_trips_optional_syntax() {
    let mut value = minimal();
    value["parameters"]["Region"]["documentation"] = "The region.".into();
    value["parameters"]["Region"]["deprecated"] =
        serde_json::json!({"message": "use Location", "since": "2024"});
    value["parameters"]["Names"] =
        serde_json::json!({"type": "stringArray", "required": true, "default": ["a", "b"]});
    value["results"][1]["documentation"] = "doc".into();
    value["conditions"][0]["argv"] = serde_json::json!([
        {"fn": "getAttr", "argv": [{"ref": "Region"}, "a.b[0].c[-1]"]},
        "{{literal}} {Region#[1]}",
        7,
        -3
    ]);
    let node = trait_node(value);
    let rules = EndpointBdd::from_node(&node).unwrap();
    assert_eq!(rules.to_node(), node);
    let region = rules.parameters().get("Region").unwrap();
    assert_eq!(region.documentation(), Some("The region."));
    let deprecated = region.deprecated().unwrap();
    assert_eq!(
        (deprecated.message(), deprecated.since()),
        (Some("use Location"), Some("2024"))
    );
    assert_eq!(
        rules.parameters().get("Names").unwrap().default(),
        Some(&ParameterValue::StringArray(vec!["a".into(), "b".into()]))
    );
    assert_eq!(rules.results()[1].documentation(), Some("doc"));

    let arguments = rules.conditions()[0].arguments();
    let ExpressionKind::Literal(Literal::String(template)) = arguments[1].kind() else {
        panic!()
    };
    assert_eq!(
        template.segments(),
        [
            TemplateSegment::Literal("{literal} ".into()),
            TemplateSegment::Reference {
                name: "Region".into(),
                path: Box::new([PathSegment::Index(1)])
            },
        ]
    );
    assert!(matches!(
        arguments[3].kind(),
        ExpressionKind::Literal(Literal::Integer(-3))
    ));
}

#[test]
fn unknown_functions_and_root_members_are_preserved_or_tolerated() {
    let mut value = minimal();
    value["conditions"][0]["fn"] = "example.custom".into();
    value["extension"] = "ignored".into();
    let rules = decode(value).unwrap();
    assert_eq!(rules.conditions()[0].function_name(), "example.custom");
    assert!(rules
        .to_node()
        .as_object()
        .unwrap()
        .get("extension")
        .is_none());
}

#[test]
fn decodes_complemented_edges_and_non_node_roots() {
    // Synthetic shapes absent from the AWS corpus: complemented edges and terminal edges.
    let mut value = minimal();
    value["nodes"] = nodes(&[[0, -3, 1], [1, R + 1, -1]]).into();
    let node = trait_node(value);
    let rules = EndpointBdd::from_node(&node).unwrap();
    assert_eq!(rules.to_node(), node);
    let BddRef::Node(root) = rules.bdd().root() else {
        panic!()
    };
    let BddRef::Node(high) = rules.bdd().node(root).high() else {
        panic!()
    };
    assert!(high.is_complemented());
    assert_eq!(rules.bdd().node(root).low(), BddRef::Terminal(true));

    for root in [-1, 1, R, R + 1, R + 2] {
        let mut value = minimal();
        value["root"] = root.into();
        let node = trait_node(value);
        let rules = EndpointBdd::from_node(&node).unwrap();
        assert_eq!(rules.to_node(), node, "root {root}");
    }
}

#[test]
fn rejects_malformed_root_members() {
    expect_error(
        |v| {
            v.as_object_mut().unwrap().remove("nodes");
        },
        "missing required member `nodes`",
        "",
    );
    expect_error(
        |v| v["version"] = "1.0".into(),
        "version 1.1 or later",
        "/version",
    );
    expect_error(
        |v| v["version"] = "1".into(),
        "at least two numeric",
        "/version",
    );
    expect_error(
        |v| v["version"] = 1.1.into(),
        "at least two numeric",
        "/version",
    );
    expect_error(|v| v["root"] = 2.5.into(), "signed 32-bit", "/root");
    expect_error(
        |v| v["root"] = (1i64 << 32).into(),
        "signed 32-bit",
        "/root",
    );
    expect_error(|v| v["nodeCount"] = 0.into(), "at least 1", "/nodeCount");
    expect_error(|v| v["nodeCount"] = 4.into(), "requires 64", "/nodes");
    expect_error(
        |v| v["nodeCount"] = i32::MAX.into(),
        "exceeding the limit",
        "/nodeCount",
    );
    expect_error(|v| v["nodes"] = 3.into(), "base64 string", "/nodes");
    expect_error(
        |v| v["nodes"] = "!".repeat(48).into(),
        "standard base64",
        "/nodes",
    );
    // URL-safe alphabet is rejected.
    expect_error(
        |v| {
            v["nodes"] = nodes(&[[0, 3, R + 2], [1, -1, -1]])
                .replace('/', "_")
                .into()
        },
        "standard base64",
        "/nodes",
    );
}

#[test]
fn rejects_malformed_graphs() {
    expect_error(|v| v["root"] = 0.into(), "invalid reference 0", "/root");
    expect_error(|v| v["root"] = (-2).into(), "complemented node", "/root");
    expect_error(
        |v| v["root"] = (R + 3).into(),
        "references result 3",
        "/root",
    );
    expect_error(
        |v| v["nodes"] = nodes(&[[0, 3, R + 2], [2, 1, -1]]).into(),
        "condition index 2 is out of range",
        "/nodes",
    );
    expect_error(
        |v| v["nodes"] = nodes(&[[0, 3, -R]]).into(),
        "has 32 base64 characters, but `nodeCount` 3 requires 48",
        "/nodes",
    );
    expect_error(
        |v| v["nodes"] = nodes(&[[0, 3, -R - 1], [1, 1, -1]]).into(),
        "complemented result",
        "/nodes",
    );
    expect_error(
        |v| v["nodes"] = nodes(&[[1, 3, 1], [0, 1, -1]]).into(),
        "is not greater than 1",
        "/nodes",
    );
    expect_error(
        |v| {
            v["nodes"] = nodes(&[[0, 3, 1], [1, 1, -1]])
                .replacen("/////wAAAAH", "AAAAAAAAAAH", 1)
                .into()
        },
        "terminal sentinel",
        "/nodes",
    );
}

#[test]
fn rejects_malformed_parameters() {
    expect_error(
        |v| v["parameters"]["Region"]["extra"] = true.into(),
        "unknown parameter member `extra`",
        "/parameters/Region/extra",
    );
    expect_error(
        |v| v["parameters"]["Region"]["type"] = "integer".into(),
        "`string`, `boolean`, or `stringArray`",
        "/parameters/Region/type",
    );
    expect_error(
        |v| v["parameters"]["UseFIPS"]["default"] = "no".into(),
        "must be a boolean value",
        "/parameters/UseFIPS/default",
    );
    expect_error(
        |v| v["parameters"]["UseFIPS"]["required"] = false.into(),
        "has a default but is not required",
        "/parameters/UseFIPS/default",
    );
    expect_error(
        |v| v["parameters"]["Region"]["deprecated"] = serde_json::json!({"when": "x"}),
        "unknown deprecated member `when`",
        "/parameters/Region/deprecated/when",
    );
}

#[test]
fn rejects_malformed_expressions() {
    expect_error(
        |v| v["conditions"][0]["argv"][0] = serde_json::json!({"ref": "A", "fn": "f"}),
        "both `ref` and `fn`",
        "/conditions/0/argv/0",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = serde_json::json!({"ref": ""}),
        "non-empty string",
        "/conditions/0/argv/0/ref",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = serde_json::json!({"fn": "f"}),
        "missing `argv`",
        "/conditions/0/argv/0",
    );
    expect_error(
        |v| v["conditions"][0]["assign"] = 1.into(),
        "`assign` must be a string",
        "/conditions/0/assign",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = "{Region".into(),
        "unclosed `{`",
        "/conditions/0/argv/0",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = "a}b".into(),
        "unmatched `}`",
        "/conditions/0/argv/0",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = "{A#b[x]}".into(),
        "invalid attribute path",
        "/conditions/0/argv/0",
    );
    expect_error(
        |v| {
            v["conditions"][0]["argv"] =
                serde_json::json!([{"fn": "getAttr", "argv": [{"ref": "A"}, "a..b"]}])
        },
        "invalid attribute path",
        "/conditions/0/argv/0/argv/1",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = (1i64 << 31).into(),
        "signed 32-bit",
        "/conditions/0/argv/0",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = 1.5.into(),
        "signed 32-bit",
        "/conditions/0/argv/0",
    );
    expect_error(
        |v| v["conditions"][0]["argv"][0] = serde_json::Value::Null,
        "must not be null",
        "/conditions/0/argv/0",
    );
}

#[test]
fn rejects_malformed_results() {
    expect_error(
        |v| v["results"][0]["conditions"] = serde_json::json!([{"fn": "isSet", "argv": []}]),
        "must have no conditions",
        "/results/0/conditions",
    );
    expect_error(
        |v| v["results"][0]["type"] = "tree".into(),
        "tree rules are not allowed",
        "/results/0/type",
    );
    expect_error(
        |v| v["results"][1]["endpoint"] = serde_json::json!({"url": "x"}),
        "must not have `endpoint`",
        "/results/1/endpoint",
    );
    expect_error(
        |v| v["results"][0]["endpoint"]["properties"]["p"] = serde_json::json!({"x": null}),
        "must not be null",
        "/results/0/endpoint/properties/p/x",
    );
    expect_error(
        |v| v["results"][0]["endpoint"]["headers"]["x-a"] = "b".into(),
        "array of header values",
        "/results/0/endpoint/headers/x-a",
    );
}

#[test]
fn enforces_limits() {
    let limit = |limits: EndpointRulesLimits, message: &str| {
        let err = decode_with(minimal(), &limits).unwrap_err();
        assert!(err.message().contains(message), "{}", err.message());
    };
    let default = EndpointRulesLimits::default;
    limit(default().with_max_parameters(1), "`parameters` has 2");
    limit(default().with_max_conditions(1), "`conditions` has 2");
    limit(default().with_max_results(1), "`results` has 2");
    limit(default().with_max_nodes(2), "`nodeCount` has 3");
    limit(default().with_max_expression_depth(2), "nesting exceeds");
    limit(
        default().with_max_expression_nodes(3),
        "limit of 3 expressions",
    );
    limit(
        default().with_max_template_segments(3),
        "limit of 3 template",
    );

    // Built in code: the JSON loader's own depth limit would reject this first.
    let mut deep = Node::from("x");
    for _ in 0..1_000 {
        deep = Node::from(vec![deep]);
    }
    let mut value = to_node(&minimal());
    let NodeValue::Object(root) = value.value() else {
        panic!()
    };
    let mut root = root.clone();
    let condition =
        NodeObject::from_iter([("fn", Node::from("f")), ("argv", Node::from(vec![deep]))]);
    root.insert("conditions", Node::from(vec![Node::from(condition)]));
    value = Node::from(root);
    let err = EndpointBdd::from_node(&value).unwrap_err();
    assert!(
        err.message().contains("nesting exceeds the limit of 100"),
        "{}",
        err.message()
    );
}

/// Converts JSON to a node without source locations.
fn to_node(value: &serde_json::Value) -> Node {
    use serde_json::Value;
    match value {
        Value::Null => Node::null(),
        Value::Bool(b) => Node::from(*b),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => Node::from(i),
            (_, Some(u)) => Node::from(u),
            _ => Node::from(Number::from_f64(n.as_f64().unwrap()).unwrap()),
        },
        Value::String(s) => Node::from(s.as_str()),
        Value::Array(items) => Node::from(items.iter().map(to_node).collect::<Vec<_>>()),
        Value::Object(members) => Node::from(
            members
                .iter()
                .map(|(k, v)| (k.clone(), to_node(v)))
                .collect::<NodeObject>(),
        ),
    }
}

#[test]
fn validates_models_with_the_trait_codec_validator() {
    let mut value = minimal();
    value["root"] = 0.into();
    let model = serde_json::json!({
        "smithy": "2.0",
        "shapes": {"ex#S": {"type": "service", "traits": {"smithy.rules#endpointBdd": value}}}
    });
    let err = Model::assembler()
        .add_json_str("model.json", model.to_string())
        .add_validator(validation::trait_codec::<EndpointBdd>())
        .assemble()
        .into_result()
        .unwrap_err();
    let diagnostic = err.diagnostics().errors().next().unwrap();
    assert_eq!(
        diagnostic.source_location().unwrap().pointer(),
        Some("/shapes/ex#S/traits/smithy.rules#endpointBdd/root")
    );
}
