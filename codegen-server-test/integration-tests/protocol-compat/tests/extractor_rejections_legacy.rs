/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Legacy observations for the selected-protocol extractor rejection change.
#[path = "support/extractor_rejections.rs"]
mod harness;
use harness::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

macro_rules! parity {
    ($test:ident, $wire:expr, $name:literal, $legacy:ident, $schema:ident) => {
        #[tokio::test]
        async fn $test() {
            let legacy_calls = Arc::new(AtomicUsize::new(0));
            let schema_calls = Arc::new(AtomicUsize::new(0));
            let legacy = $legacy::service(legacy_calls.clone());
            let schema = $schema::service(schema_calls.clone());
            for kind in REJECTIONS {
                let observed = send(legacy.clone(), request($wire, $name, kind)).await;
                assert_eq!(
                    observed,
                    observed_runtime_rejection($wire, kind),
                    "legacy {kind}"
                );
                assert_eq!(
                    send(schema.clone(), request($wire, $name, kind)).await,
                    observed,
                    "schema {kind}"
                );
            }
            assert_eq!(legacy_calls.load(Ordering::SeqCst), 0);
            assert_eq!(schema_calls.load(Ordering::SeqCst), 0);
            let observed = send(legacy, request($wire, $name, "success")).await;
            assert_eq!(observed.status, 200);
            assert_eq!(
                send(schema, request($wire, $name, "success")).await,
                observed
            );
            assert_eq!(legacy_calls.load(Ordering::SeqCst), 1);
            assert_eq!(schema_calls.load(Ordering::SeqCst), 1);
        }
    };
}
parity!(
    rest_json_extractor_responses_match_generated_legacy,
    Wire::Rest,
    "ReviewRest",
    legacy_rest,
    schema_rest
);
parity!(
    cbor_extractor_responses_match_generated_legacy,
    Wire::Cbor,
    "ReviewCbor",
    legacy_cbor,
    schema_cbor
);
parity!(
    aws_json_extractor_responses_match_generated_legacy,
    Wire::AwsJson,
    "ReviewAwsJson",
    legacy_json,
    schema_json
);

#[tokio::test]
async fn streaming_rest_json_extractor_rejections_match_generated_legacy() {
    for kind in REJECTIONS {
        let mut legacy_request = request(Wire::Rest, "ReviewRest", kind);
        *legacy_request.uri_mut() = "/download".parse().unwrap();
        let mut schema_request = request(Wire::Rest, "ReviewRest", kind);
        *schema_request.uri_mut() = "/download".parse().unwrap();
        let observed = send(legacy_rest::streaming_service(), legacy_request).await;
        assert_eq!(observed, observed_runtime_rejection(Wire::Rest, kind));
        assert_eq!(
            send(schema_rest::streaming_service(), schema_request).await,
            observed
        );
    }
}

#[tokio::test]
async fn legacy_custom_response_and_schema_default_conversion_match() {
    macro_rules! service {
        ($sdk:ident, [$($builder:tt)*]) => {{
            use $sdk::{ReviewRest, ReviewRestConfig, input::GetWidgetInput, output::GetWidgetOutput, error::GetWidgetError};
            ReviewRest::builder $($builder)* (ReviewRestConfig::builder().build())
                .get_widget(|_: GetWidgetInput, _: CustomCaller| async { panic!("rejected custom extractor reached handler"); #[allow(unreachable_code)] Ok::<_, GetWidgetError>(GetWidgetOutput { id: None }) })
                .build_unchecked()
        }};
    }
    use aws_smithy_http_server::body::Body;
    assert_custom_rejection(
        service!(compat_rest_legacy_server_sdk, [::<Body, _, _, _>]),
        request(Wire::Rest, "ReviewRest", "custom"),
    )
    .await;
    assert_custom_rejection(
        service!(compat_rest_server_sdk, []),
        request(Wire::Rest, "ReviewRest", "custom"),
    )
    .await;
}

#[tokio::test]
async fn generic_custom_response_is_preserved_on_both_protocols() {
    use compat_rest_cbor_server_sdk::{
        error::GetWidgetError, input::GetWidgetInput, output::GetWidgetOutput, ReviewRestCbor,
        ReviewRestCborConfig,
    };
    let service = ReviewRestCbor::builder(ReviewRestCborConfig::builder().build())
        .get_widget(|_: GetWidgetInput, _: CustomCaller| async {
            panic!("rejected custom extractor reached handler");
            #[allow(unreachable_code)]
            Ok::<_, GetWidgetError>(GetWidgetOutput { id: None })
        })
        .build_unchecked();
    for wire in [Wire::Rest, Wire::Cbor, Wire::Rest, Wire::Cbor] {
        assert_custom_rejection(service.clone(), request(wire, "ReviewRestCbor", "custom")).await;
    }
}

#[tokio::test]
async fn required_extension_rejects_and_optional_extension_reaches_handler() {
    use aws_smithy_http_server::Extension;
    use compat_rest_cbor_server_sdk::{
        error::GetWidgetError, input::GetWidgetInput, output::GetWidgetOutput, ReviewRestCbor,
        ReviewRestCborConfig,
    };
    let required = ReviewRestCbor::builder(ReviewRestCborConfig::builder().build())
        .get_widget(|_: GetWidgetInput, _: Extension<String>| async {
            panic!("missing required extension reached handler");
            #[allow(unreachable_code)]
            Ok::<_, GetWidgetError>(GetWidgetOutput { id: None })
        })
        .build_unchecked();
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    let optional = ReviewRestCbor::builder(ReviewRestCborConfig::builder().build())
        .get_widget(
            move |input: GetWidgetInput, value: Option<Extension<String>>| {
                assert!(value.is_none());
                handler_calls.fetch_add(1, Ordering::SeqCst);
                async move { Ok::<_, GetWidgetError>(GetWidgetOutput { id: Some(input.id) }) }
            },
        )
        .build_unchecked();
    for wire in [Wire::Rest, Wire::Cbor] {
        let reply = send(required.clone(), request(wire, "ReviewRestCbor", "success")).await;
        assert_eq!(reply.status, 500);
        assert!(reply.headers.is_empty());
        assert!(reply.body.is_empty());
        assert!(reply.runtime_error.is_none());
        assert_eq!(
            send(optional.clone(), request(wire, "ReviewRestCbor", "success"))
                .await
                .status,
            200
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

parity!(
    aws_json_11_extractor_responses_match_generated_legacy,
    Wire::AwsJson11,
    "ReviewAwsJson11",
    legacy_json11,
    schema_json11
);
parity!(
    rest_xml_extractor_responses_match_generated_legacy,
    Wire::Xml,
    "ReviewXml",
    legacy_xml,
    schema_xml
);
