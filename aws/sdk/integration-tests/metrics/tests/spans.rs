/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::collections::HashMap;
use std::fmt;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::Subscriber;
use tracing_fluent_assertions::{AssertionRegistry, AssertionsLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;
mod utils;
use utils::{make_config, make_ddb_call, make_s3_call};

#[tokio::test]
async fn top_level_spans_exist_with_correct_attributes() {
    let s3_top_level: fn() -> Box<dyn Visit + 'static> = || Box::new(S3TestVisitor);
    let ddb_top_level: fn() -> Box<dyn Visit + 'static> = || Box::new(DdbTestVisitor);
    let subscriber = tracing_subscriber::registry::Registry::default().with(TestLayer {
        visitor_factories: HashMap::from([
            ("s3.GetObject", s3_top_level),
            ("dynamodb.GetItem", ddb_top_level),
        ]),
    });
    let _ = tracing::subscriber::set_default(subscriber);

    let config = make_config(false);
    make_s3_call(&config).await;
    make_ddb_call(&config).await;
}

#[tokio::test]
async fn try_attempt_spans_emitted_per_retry() {
    let assertion_registry = AssertionRegistry::default();
    let base_subscriber = tracing_subscriber::Registry::default();
    let subscriber = base_subscriber.with(AssertionsLayer::new(&assertion_registry));
    let _guard = tracing::subscriber::set_default(subscriber);

    let two_try_attempts = assertion_registry
        .build()
        .with_name("try_attempt")
        .with_span_field("attempt_number")
        .was_closed_exactly(2)
        .finalize();

    let config = make_config(true);
    make_s3_call(&config).await;

    two_try_attempts.assert();
}

#[tokio::test]
async fn all_expected_operation_spans_emitted_with_correct_nesting() {
    let assertion_registry = AssertionRegistry::default();
    let base_subscriber = tracing_subscriber::Registry::default();
    let subscriber = base_subscriber.with(AssertionsLayer::new(&assertion_registry));
    let _guard = tracing::subscriber::set_default(subscriber);

    const OPERATION_NAME: &str = "s3.GetObject";
    const INVOKE: &str = "invoke";
    const TRY_OP: &str = "try_op";
    const TRY_ATTEMPT: &str = "try_attempt";

    let apply_configuration = assertion_registry
        .build()
        .with_name("apply_configuration")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .was_closed_exactly(1)
        .finalize();

    let serialization = assertion_registry
        .build()
        .with_name("serialization")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .with_parent_name(TRY_OP)
        .was_closed_exactly(1)
        .finalize();

    let orchestrate_endpoint = assertion_registry
        .build()
        .with_name("orchestrate_endpoint")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .with_parent_name(TRY_OP)
        .with_parent_name(TRY_ATTEMPT)
        .was_closed_exactly(1)
        .finalize();

    let lazy_load_identity = assertion_registry
        .build()
        .with_name("lazy_load_identity")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .with_parent_name(TRY_OP)
        .with_parent_name(TRY_ATTEMPT)
        .was_closed_exactly(1)
        .finalize();

    let deserialize_streaming = assertion_registry
        .build()
        .with_name("deserialize_streaming")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .with_parent_name(TRY_OP)
        .with_parent_name(TRY_ATTEMPT)
        .was_closed_exactly(1)
        .finalize();

    let deserialization = assertion_registry
        .build()
        .with_name("deserialization")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .with_parent_name(TRY_OP)
        .with_parent_name(TRY_ATTEMPT)
        .was_closed_exactly(1)
        .finalize();

    let try_attempt = assertion_registry
        .build()
        .with_name(TRY_ATTEMPT)
        .with_span_field("attempt_number")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .with_parent_name(TRY_OP)
        .was_closed_exactly(1)
        .finalize();

    let finally_attempt = assertion_registry
        .build()
        .with_name("finally_attempt")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .with_parent_name(TRY_OP)
        .was_closed_exactly(1)
        .finalize();

    let try_op = assertion_registry
        .build()
        .with_name(TRY_OP)
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .was_closed_exactly(1)
        .finalize();

    let finally_op = assertion_registry
        .build()
        .with_name("finally_op")
        .with_parent_name(OPERATION_NAME)
        .with_parent_name(INVOKE)
        .was_closed_exactly(1)
        .finalize();

    let invoke = assertion_registry
        .build()
        .with_name(INVOKE)
        .with_parent_name(OPERATION_NAME)
        .was_closed_exactly(1)
        .finalize();

    let operation = assertion_registry
        .build()
        .with_name(OPERATION_NAME)
        .was_closed_exactly(1)
        .finalize();

    let config = make_config(false);
    make_s3_call(&config).await;

    apply_configuration.assert();
    serialization.assert();
    orchestrate_endpoint.assert();
    lazy_load_identity.assert();
    deserialize_streaming.assert();
    deserialization.assert();
    try_attempt.assert();
    finally_attempt.assert();
    try_op.assert();
    finally_op.assert();
    invoke.assert();
    operation.assert();
}

struct TestLayer<F: Fn() -> Box<dyn Visit> + 'static> {
    visitor_factories: HashMap<&'static str, F>,
}

impl<S, F> Layer<S> for TestLayer<F>
where
    S: Subscriber,
    S: for<'lookup> LookupSpan<'lookup>,
    F: Fn() -> Box<dyn Visit>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();
        let span_name = span.metadata().name();

        // Assert that any top level spans are the operation spans
        if span.parent().is_none() {
            assert!(
                span_name == "dynamodb.GetItem" || span_name == "s3.GetObject",
                "Encountered unexpected top level span {span_name}"
            )
        }

        for (asserted_span, visitor_factory) in &self.visitor_factories {
            if &span_name == asserted_span {
                let mut visitor = visitor_factory();
                attrs.values().record(&mut *visitor);
            }
        }
    }
}

struct S3TestVisitor;

impl Visit for S3TestVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let field_name = field.name();
        let field_value = format!("{value:?}").replace("\"", "");
        if field_name == "rpc.system" {
            assert_eq!("aws-api".to_string(), field_value);
        } else if field_name == "rpc.service" {
            assert_eq!("s3".to_string(), field_value);
        } else if field_name == "rpc.method" {
            assert_eq!("GetObject".to_string(), field_value);
        } else if field_name == "sdk_invocation_id" {
            let num: u32 = field_value.parse().unwrap();
            assert!(1_000_000 <= num);
            assert!(num < 10_000_000);
        } else {
            panic!("Unknown attribute present on top level operation span - {field_name}: {field_value}")
        }
    }
}

struct DdbTestVisitor;

impl Visit for DdbTestVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let field_name = field.name();
        let field_value = format!("{value:?}").replace("\"", "");
        if field_name == "rpc.system" {
            assert_eq!("aws-api".to_string(), field_value);
        } else if field_name == "rpc.service" {
            assert_eq!("dynamodb".to_string(), field_value);
        } else if field_name == "rpc.method" {
            assert_eq!("GetItem".to_string(), field_value);
        } else if field_name == "sdk_invocation_id" {
            let num: u32 = field_value.parse().unwrap();
            assert!(1_000_000 <= num);
            assert!(num < 10_000_000);
        } else {
            panic!("Unknown attribute present on top level operation span - {field_name}: {field_value}")
        }
    }
}
