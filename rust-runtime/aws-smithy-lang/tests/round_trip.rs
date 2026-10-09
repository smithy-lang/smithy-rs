/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Writer output is independent of semantically unordered input order.

use aws_smithy_lang::{Model, ModelWriter};
use proptest::prelude::*;

fn object(entries: &[String]) -> String {
    format!("{{{}}}", entries.join(","))
}

fn refs(ids: &[&str]) -> String {
    let refs: Vec<_> = ids
        .iter()
        .map(|id| format!(r#"{{"target":"{id}"}}"#))
        .collect();
    format!("[{}]", refs.join(","))
}

/// A permutation of everything the writer treats as unordered.
#[derive(Debug, Clone)]
struct Order {
    shapes: Vec<usize>,
    top_level: Vec<usize>,
    traits: Vec<usize>,
    trait_value: Vec<usize>,
    operations: Vec<usize>,
    errors: Vec<usize>,
    identifiers: Vec<usize>,
    shape_keys: Vec<usize>,
}

fn permute<T: Clone>(items: &[T], order: &[usize]) -> Vec<T> {
    order.iter().map(|&i| items[i].clone()).collect()
}

/// Builds the same model with every unordered collection in the given order.
fn build(order: &Order) -> String {
    let operations = permute(&["a#Op1", "a#Op2", "a#Op3"], &order.operations);
    let errors = permute(&["a#Err1", "a#Err2"], &order.errors);
    let trait_value = object(&permute(
        &[
            r#""z":[3,1,2]"#.to_owned(),
            r#""a":{"q":1,"b":2}"#.to_owned(),
            r#""m":null"#.to_owned(),
        ],
        &order.trait_value,
    ));
    let traits = object(&permute(
        &[
            format!(r#""a#custom":{trait_value}"#),
            r#""smithy.api#documentation":"d""#.to_owned(),
            r#""smithy.api#input":{}"#.to_owned(),
        ],
        &order.traits,
    ));
    let identifiers = object(&permute(
        &[
            r#""b":{"target":"a#Str"}"#.to_owned(),
            r#""a":{"target":"a#Str"}"#.to_owned(),
        ],
        &order.identifiers,
    ));
    // Member order is significant and therefore fixed.
    let members = r#""members":{"z":{"target":"a#Str"},"a":{"target":"a#Str"}}"#;
    let input = object(&permute(
        &[
            r#""type":"structure""#.to_owned(),
            members.to_owned(),
            format!(r#""traits":{traits}"#),
        ],
        &order.shape_keys,
    ));
    let shapes = [
        format!(
            r#""a#Svc":{{"type":"service","operations":{},"errors":{}}}"#,
            refs(&operations),
            refs(&errors)
        ),
        format!(
            r#""a#Op1":{{"type":"operation","input":{{"target":"a#In"}},"errors":{}}}"#,
            refs(&errors)
        ),
        r#""a#Op2":{"type":"operation"}"#.to_owned(),
        r#""a#Op3":{"type":"operation","output":{"target":"smithy.api#Unit"}}"#.to_owned(),
        format!(r#""a#In":{input}"#),
        r#""a#Str":{"type":"string"}"#.to_owned(),
        r#""a#Err1":{"type":"structure","traits":{"smithy.api#error":"client"}}"#.to_owned(),
        r#""a#Err2":{"type":"structure","traits":{"smithy.api#error":"server"}}"#.to_owned(),
        format!(
            r#""a#R":{{"type":"resource","identifiers":{identifiers},"operations":{}}}"#,
            refs(&operations)
        ),
    ];
    let top_level = permute(
        &[
            r#""smithy":"2.0""#.to_owned(),
            r#""metadata":{"y":[2,1],"b":{"d":1,"c":2}}"#.to_owned(),
            format!(r#""shapes":{}"#, object(&permute(&shapes, &order.shapes))),
        ],
        &order.top_level,
    );
    object(&top_level)
}

fn shuffled(n: usize) -> impl Strategy<Value = Vec<usize>> {
    Just((0..n).collect::<Vec<_>>()).prop_shuffle()
}

fn arb_order() -> impl Strategy<Value = Order> {
    (
        shuffled(9),
        shuffled(3),
        shuffled(3),
        shuffled(3),
        shuffled(3),
        shuffled(2),
        shuffled(2),
        shuffled(3),
    )
        .prop_map(
            |(
                shapes,
                top_level,
                traits,
                trait_value,
                operations,
                errors,
                identifiers,
                shape_keys,
            )| Order {
                shapes,
                top_level,
                traits,
                trait_value,
                operations,
                errors,
                identifiers,
                shape_keys,
            },
        )
}

fn identity() -> Order {
    let id = |n| (0..n).collect();
    Order {
        shapes: id(9),
        top_level: id(3),
        traits: id(3),
        trait_value: id(3),
        operations: id(3),
        errors: id(2),
        identifiers: id(2),
        shape_keys: id(3),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn output_is_independent_of_unordered_input(order in arb_order()) {
        let baseline = Model::from_json_str("baseline", &build(&identity())).unwrap();
        let model = Model::from_json_str("shuffled", &build(&order)).unwrap();
        for writer in [ModelWriter::new(), ModelWriter::new().pretty(true)] {
            prop_assert_eq!(writer.to_string(&model).unwrap(), writer.to_string(&baseline).unwrap());
        }
        prop_assert!(model.equivalent(&baseline));
    }
}

#[test]
fn member_and_array_order_are_preserved() {
    let model = Model::from_json_str("t", &build(&identity())).unwrap();
    let out = ModelWriter::new().to_string(&model).unwrap();
    assert!(out.contains(r#""members":{"z":{"target":"a#Str"},"a":{"target":"a#Str"}}"#));
    assert!(out.contains(r#""z":[3,1,2]"#));
    assert!(out.contains(r#""y":[2,1]"#));
    assert!(out.contains(r#""b":{"c":2,"d":1}"#));
}
