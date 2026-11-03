/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Static validation of endpoint rules and their context-binding traits.

use aws_smithy_lang::diagnostic::{Diagnostic, DiagnosticCode, Severity};
use aws_smithy_lang::endpoint_rules::{
    self, ClientContextParams, ContextParam, EndpointBdd, OperationContextParams, ParameterType,
    ParameterValue, StaticContextParams,
};
use aws_smithy_lang::traits::TraitCodec;
use aws_smithy_lang::{Model, ShapeId};
use serde_json::{json, Value};
use std::path::PathBuf;

const R: i32 = 100_000_000;
const TRAIT: &str = "/shapes/ex#S/traits/smithy.rules#endpointBdd";

fn nodes(nodes: &[[i32; 3]]) -> String {
    let mut bytes = Vec::new();
    for value in [-1, 1, -1]
        .into_iter()
        .chain(nodes.iter().flatten().copied())
    {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// Rules with a built-in `Region`, a client-configured `Accelerate`, and an operation-bound
/// required `Bucket`.
fn rules() -> Value {
    json!({
        "version": "1.1",
        "parameters": {
            "Region": {"type": "string", "builtIn": "AWS::Region", "documentation": "d"},
            "Accelerate": {"type": "boolean", "required": true, "default": false, "documentation": "d"},
            "Bucket": {"type": "string", "required": true, "documentation": "d"}
        },
        "conditions": [
            {"fn": "isSet", "argv": [{"ref": "Region"}]},
            {"fn": "aws.partition", "argv": [{"ref": "Region"}], "assign": "Partition"},
            {"fn": "booleanEquals", "argv": [{"ref": "Accelerate"}, true]}
        ],
        "results": [
            {
                "type": "endpoint",
                "conditions": [],
                "endpoint": {
                    "url": "https://{Bucket}.{Region}.{Partition#dnsSuffix}",
                    "properties": {"authSchemes": [{"name": "sigv4", "signingRegion": "{Region}"}]},
                    "headers": {}
                }
            },
            {"type": "error", "conditions": [], "error": "no region"}
        ],
        "root": 2,
        "nodeCount": 4,
        "nodes": nodes(&[[0, 3, R + 2], [1, 4, R], [2, R + 1, R + 1]])
    })
}

/// A service using `rules`, with one operation binding `Bucket` through its input.
fn model(rules: Value) -> Value {
    json!({
        "smithy": "2.0",
        "shapes": {
            "ex#S": {
                "type": "service",
                "operations": [{"target": "ex#Get"}],
                "traits": {
                    "smithy.rules#endpointBdd": rules,
                    "smithy.rules#clientContextParams": {
                        "Accelerate": {"type": "boolean", "documentation": "d"}
                    }
                }
            },
            "ex#Get": {
                "type": "operation",
                "input": {"target": "ex#GetInput"},
                "traits": {
                    "smithy.rules#staticContextParams": {"Accelerate": {"value": false}}
                }
            },
            "ex#GetInput": {
                "type": "structure",
                "members": {
                    "bucket": {
                        "target": "smithy.api#String",
                        "traits": {"smithy.rules#contextParam": {"name": "Bucket"}}
                    },
                    "count": {"target": "smithy.api#Integer"}
                }
            }
        }
    })
}

/// The diagnostics reported by the endpoint rules validator.
fn validate(model: Value) -> Vec<Diagnostic> {
    Model::assembler()
        .add_json_str("model.json", model.to_string())
        .add_validator(endpoint_rules::validator())
        .assemble()
        .diagnostics()
        .iter()
        .filter(|d| d.validator() == Some("EndpointRules"))
        .cloned()
        .collect()
}

/// The one diagnostic reported for `model`, which must contain `message` and be located at
/// `pointer`.
fn expect_one(model: Value, severity: Severity, message: &str, pointer: &str) -> Diagnostic {
    let diagnostics = validate(model);
    let [diagnostic] = &diagnostics[..] else {
        panic!("expected one diagnostic, found {diagnostics:#?}");
    };
    assert_eq!(diagnostic.severity(), severity, "{diagnostic}");
    assert!(diagnostic.message().contains(message), "{diagnostic}");
    assert_eq!(
        diagnostic.source_location().and_then(|l| l.pointer()),
        Some(pointer),
        "{diagnostic}"
    );
    assert_eq!(diagnostic.shape_id(), Some(&ShapeId::from_static("ex#S")));
    diagnostic.clone()
}

fn expect_rule_error(mutate: impl FnOnce(&mut Value), message: &str, pointer: &str) {
    let mut value = rules();
    mutate(&mut value);
    let diagnostic = expect_one(
        model(value),
        Severity::Error,
        message,
        &format!("{TRAIT}{pointer}"),
    );
    assert_eq!(diagnostic.code(), DiagnosticCode::InvalidTrait);
}

#[test]
fn valid_rules_have_no_diagnostics() {
    assert_eq!(validate(model(rules())), []);
}

#[test]
fn corpus_has_no_diagnostics() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../aws/sdk/aws-models");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    let mut checked = 0;
    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        if !text.contains("smithy.rules#endpointBdd") {
            continue;
        }
        checked += 1;
        let validated = Model::assembler()
            .add_json_str(path.display().to_string(), text)
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
    assert!(checked >= 20, "found {checked} endpointBdd models");
}

#[test]
fn reports_assignment_conflicts() {
    expect_rule_error(
        |v| v["conditions"][2]["assign"] = "Partition".into(),
        "condition 2 assigns `Partition`, already assigned by condition 1",
        "/conditions/2",
    );
    expect_rule_error(
        |v| v["conditions"][0]["assign"] = "Region".into(),
        "condition 0 assigns `Region`, which is a parameter name",
        "/conditions/0",
    );
}

#[test]
fn reports_references_out_of_scope() {
    expect_rule_error(
        |v| v["conditions"][0]["argv"][0] = json!({"ref": "Missing"}),
        "condition 0 references `Missing`, which is not a parameter or an earlier assignment",
        "/conditions/0/argv/0",
    );
    expect_rule_error(
        |v| v["conditions"][0]["argv"][0] = json!({"ref": "Partition"}),
        "assigned by condition 1 and is never bound when condition 0 is tested",
        "/conditions/0/argv/0",
    );
    expect_rule_error(
        |v| v["conditions"][1]["argv"][0] = json!({"ref": "Partition"}),
        "condition 1 references `Partition`, which is assigned by condition 1",
        "/conditions/1/argv/0",
    );
    // Template references inside function arguments are checked too.
    expect_rule_error(
        |v| v["conditions"][0]["argv"][0] = "{Missing}".into(),
        "references `Missing`",
        "/conditions/0/argv/0",
    );
    expect_rule_error(
        |v| v["results"][1]["error"] = "no {Nope}".into(),
        "result 1 references `Nope`",
        "/results/1/error",
    );
    // References inside literal properties are reported at the endpoint.
    expect_rule_error(
        |v| v["results"][0]["endpoint"]["properties"]["p"] = json!(["{Gone}"]),
        "result 0 references `Gone`",
        "/results/0/endpoint",
    );
}

#[test]
fn reports_url_schemes() {
    expect_rule_error(
        |v| v["results"][0]["endpoint"]["url"] = "ftp://{Region}".into(),
        "must start with `http://` or `https://`, but starts with `ftp://`",
        "/results/0/endpoint/url",
    );
    // A URL computed by a reference or function is not checked.
    let mut value = rules();
    value["results"][0]["endpoint"]["url"] = json!({"ref": "Region"});
    assert_eq!(validate(model(value)), []);
}

#[test]
fn warns_about_undocumented_parameters_and_unknown_members() {
    let mut value = rules();
    value["parameters"]["Region"]
        .as_object_mut()
        .unwrap()
        .remove("documentation");
    let diagnostic = expect_one(
        model(value),
        Severity::Warning,
        "endpoint parameter `Region` has no documentation",
        &format!("{TRAIT}/parameters/Region"),
    );
    assert_eq!(diagnostic.code(), DiagnosticCode::MissingProperty);

    let mut value = rules();
    value["extension"] = json!(1);
    let diagnostic = expect_one(
        model(value),
        Severity::Warning,
        "unknown endpointBdd member `extension` is ignored",
        &format!("{TRAIT}/extension"),
    );
    assert_eq!(diagnostic.code(), DiagnosticCode::UnknownProperty);
}

#[test]
fn reports_decode_errors() {
    let mut value = rules();
    value["root"] = 0.into();
    let diagnostic = expect_one(
        model(value),
        Severity::Error,
        "invalid `smithy.rules#endpointBdd` trait value: `root` is the invalid reference 0",
        &format!("{TRAIT}/root"),
    );
    assert_eq!(diagnostic.code(), DiagnosticCode::InvalidTrait);

    let mut model = model(rules());
    model["shapes"]["ex#GetInput"]["members"]["bucket"]["traits"]["smithy.rules#contextParam"] =
        json!({});
    // The malformed binding is reported; the unbound parameter is reported too.
    let diagnostics = validate(model);
    assert!(
        diagnostics.iter().any(|d| d.message().contains(
            "invalid `smithy.rules#contextParam` trait value: missing required member `name`"
        )),
        "{diagnostics:#?}"
    );
}

#[test]
fn reports_bindings_that_disagree_with_parameters() {
    // A binding for a name the rules do not define.
    let mut m = model(rules());
    m["shapes"]["ex#S"]["traits"]["smithy.rules#clientContextParams"]["Other"] =
        json!({"type": "string"});
    expect_one(
        m,
        Severity::Error,
        "`Other` is bound by a context parameter trait but is not an endpoint parameter",
        "/shapes/ex#S/traits/smithy.rules#clientContextParams/Other",
    );

    // A binding of the wrong type.
    let mut m = model(rules());
    m["shapes"]["ex#Get"]["traits"]["smithy.rules#staticContextParams"]["Accelerate"] =
        json!({"value": "yes"});
    let diagnostic = expect_one(
        m,
        Severity::Error,
        "`Accelerate` is bound as a string value, but the endpoint parameter is boolean",
        "/shapes/ex#Get/traits/smithy.rules#staticContextParams/Accelerate",
    );
    assert_eq!(
        diagnostic.related()[0].pointer(),
        Some(&*format!("{TRAIT}/parameters/Accelerate"))
    );

    // A `contextParam` member of an unsupported type.
    let mut m = model(rules());
    m["shapes"]["ex#GetInput"]["members"]["count"]["traits"] =
        json!({"smithy.rules#contextParam": {"name": "Bucket"}});
    expect_one(
        m,
        Severity::Error,
        "targets a integer shape; only string, enum, and boolean targets are supported",
        "/shapes/ex#GetInput/members/count/traits/smithy.rules#contextParam",
    );
}

#[test]
fn reports_unbound_parameters() {
    // `Bucket` is required with no default, so every operation must bind it.
    let mut m = model(rules());
    m["shapes"]["ex#S"]["operations"] = json!([{"target": "ex#Get"}, {"target": "ex#List"}]);
    m["shapes"]["ex#List"] = json!({"type": "operation"});
    expect_one(
        m,
        Severity::Error,
        "operation `ex#List` does not bind required endpoint parameter `Bucket`",
        "/shapes/ex#List",
    );

    // An optional parameter that nothing binds.
    let mut value = rules();
    value["parameters"]["Unused"] = json!({"type": "string", "documentation": "d"});
    expect_one(
        model(value),
        Severity::Error,
        "endpoint parameter `Unused` is neither built-in nor bound",
        &format!("{TRAIT}/parameters/Unused"),
    );
}

#[test]
fn rules_validate_without_a_model() {
    let mut value = rules();
    value["conditions"][2]["assign"] = "Partition".into();
    let model = Model::from_json_str("m.json", &model(value).to_string()).unwrap();
    let rules = model
        .expect_shape(&ShapeId::from_static("ex#S"))
        .unwrap()
        .get_trait_as::<EndpointBdd>()
        .unwrap()
        .unwrap();
    let diagnostics = rules.validate();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity(), Severity::Error);
}

#[test]
fn context_binding_codecs_decode_from_models() {
    let mut m = model(rules());
    m["shapes"]["ex#Get"]["traits"]["smithy.rules#operationContextParams"] =
        json!({"Bucket": {"path": "Items[0].Name"}});
    let model = Model::from_json_str("m.json", &m.to_string()).unwrap();
    let shape = |id| model.expect_shape(&ShapeId::from_static(id)).unwrap();

    let client = shape("ex#S")
        .get_trait_as::<ClientContextParams>()
        .unwrap()
        .unwrap();
    let accelerate = client.get("Accelerate").unwrap();
    assert_eq!(accelerate.kind(), ParameterType::Boolean);
    assert_eq!(
        accelerate.source_location().pointer(),
        Some("/shapes/ex#S/traits/smithy.rules#clientContextParams/Accelerate")
    );

    let get = shape("ex#Get");
    let constants = get.get_trait_as::<StaticContextParams>().unwrap().unwrap();
    assert_eq!(
        constants.get("Accelerate").unwrap().value(),
        &ParameterValue::Boolean(false)
    );
    let paths = get
        .get_trait_as::<OperationContextParams>()
        .unwrap()
        .unwrap();
    assert_eq!(paths.get("Bucket").unwrap().path(), "Items[0].Name");

    let input = shape("ex#GetInput");
    let member = input.members().get("bucket").unwrap();
    let param = member.get_trait_as::<ContextParam>().unwrap().unwrap();
    assert_eq!(param.name(), "Bucket");
    assert_eq!(ContextParam::from_node(&param.to_node()).unwrap(), param);
}
