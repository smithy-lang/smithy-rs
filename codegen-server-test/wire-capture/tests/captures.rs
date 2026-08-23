//! Wire-level captures for RFC appendix — assumptions register items B2, B3, B5, B6, C1, C2, C3, C6.
//!
//! Run with: cargo test -p wire-capture -- --nocapture

use aws_smithy_http_server::body::BoxBody;
use aws_smithy_http_server::response::IntoResponse;
use bytes::Bytes;
use tower::ServiceExt;
use wire_capture::{dump_cbor_diag, dump_response};

fn request(method: &str, uri: &str, headers: &[(&str, &str)], body: &[u8]) -> http::Request<BoxBody> {
    let mut builder = http::Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    builder
        .body(aws_smithy_http_server::body::boxed(http_body_util::Full::new(
            Bytes::from(body.to_vec()),
        )))
        .expect("failed to build request")
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn json11_greeting_handler(
    _input: json_rpc11::input::GreetingWithErrorsInput,
) -> Result<json_rpc11::output::GreetingWithErrorsOutput, json_rpc11::error::GreetingWithErrorsError> {
    Err(json_rpc11::error::GreetingWithErrorsError::InvalidGreeting(
        json_rpc11::error::InvalidGreeting {
            message: Some("Hi".to_owned()),
        },
    ))
}

async fn json11_host_label_handler(
    _input: json_rpc11::input::EndpointWithHostLabelOperationInput,
) -> Result<
    json_rpc11::output::EndpointWithHostLabelOperationOutput,
    json_rpc11::error::EndpointWithHostLabelOperationError,
> {
    panic!("handler should never be reached in these captures")
}

async fn json10_greeting_handler(
    _input: json_rpc10::input::GreetingWithErrorsInput,
) -> Result<json_rpc10::output::GreetingWithErrorsOutput, json_rpc10::error::GreetingWithErrorsError> {
    Err(json_rpc10::error::GreetingWithErrorsError::InvalidGreeting(
        json_rpc10::error::InvalidGreeting {
            message: Some("Hi".to_owned()),
        },
    ))
}

async fn json10_host_label_handler(
    _input: json_rpc10::input::EndpointWithHostLabelOperationInput,
) -> Result<
    json_rpc10::output::EndpointWithHostLabelOperationOutput,
    json_rpc10::error::EndpointWithHostLabelOperationError,
> {
    panic!("handler should never be reached in these captures")
}

async fn json11_simple_scalar_handler(
    _input: json_rpc11::input::SimpleScalarPropertiesInput,
) -> json_rpc11::output::SimpleScalarPropertiesOutput {
    json_rpc11::output::SimpleScalarPropertiesOutput {
        float_value: None,
        double_value: None,
    }
}

async fn cbor_greeting_handler(
    _input: rpcv2cbor::input::GreetingWithErrorsInput,
) -> Result<rpcv2cbor::output::GreetingWithErrorsOutput, rpcv2cbor::error::GreetingWithErrorsError> {
    Err(rpcv2cbor::error::GreetingWithErrorsError::InvalidGreeting(
        rpcv2cbor::error::InvalidGreeting {
            message: Some("Hi".to_owned()),
        },
    ))
}

async fn cbor_extras_simple_struct_handler(
    _input: rpcv2cbor_extras::input::SimpleStructOperationInput,
) -> Result<
    rpcv2cbor_extras::output::SimpleStructOperationOutput,
    rpcv2cbor_extras::error::SimpleStructOperationError,
> {
    panic!("handler should never be reached in these captures")
}

async fn constraints_shapes_handler(
    _input: constraints::input::ConstrainedShapesOperationInput,
) -> Result<
    constraints::output::ConstrainedShapesOperationOutput,
    constraints::error::ConstrainedShapesOperationError,
> {
    panic!("handler should never be reached in these captures")
}

async fn custom_validation_handler(
    _input: custom_validation_exception_example::input::TestOperationInput,
) -> Result<
    custom_validation_exception_example::output::TestOperationOutput,
    custom_validation_exception_example::error::TestOperationError,
> {
    panic!("handler should never be reached in these captures")
}

// ---------------------------------------------------------------------------
// Service constructors + call helpers
// ---------------------------------------------------------------------------

async fn call_json11(req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    let config = json_rpc11::JsonProtocolConfig::builder().build();
    let app = json_rpc11::JsonProtocol::builder::<BoxBody, _, _, _>(config)
        .greeting_with_errors(json11_greeting_handler)
        .endpoint_with_host_label_operation(json11_host_label_handler)
        .simple_scalar_properties(json11_simple_scalar_handler)
        .build_unchecked();
    app.oneshot(req).await.expect("service call must not fail")
}

async fn call_json10(req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    let config = json_rpc10::JsonRpc10Config::builder().build();
    let app = json_rpc10::JsonRpc10::builder::<BoxBody, _, _, _>(config)
        .greeting_with_errors(json10_greeting_handler)
        .endpoint_with_host_label_operation(json10_host_label_handler)
        .build_unchecked();
    app.oneshot(req).await.expect("service call must not fail")
}

async fn call_cbor(req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    let config = rpcv2cbor::RpcV2ProtocolConfig::builder().build();
    let app = rpcv2cbor::RpcV2Protocol::builder::<BoxBody, _, _, _>(config)
        .greeting_with_errors(cbor_greeting_handler)
        .build_unchecked();
    app.oneshot(req).await.expect("service call must not fail")
}

async fn call_cbor_extras(req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    let config = rpcv2cbor_extras::RpcV2CborServiceConfig::builder().build();
    let app = rpcv2cbor_extras::RpcV2CborService::builder::<BoxBody, _, _, _>(config)
        .simple_struct_operation(cbor_extras_simple_struct_handler)
        .build_unchecked();
    app.oneshot(req).await.expect("service call must not fail")
}

async fn call_constraints(req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    let config = constraints::ConstraintsServiceConfig::builder().build();
    let app = constraints::ConstraintsService::builder::<BoxBody, _, _, _>(config)
        .constrained_shapes_operation(constraints_shapes_handler)
        .build_unchecked();
    app.oneshot(req).await.expect("service call must not fail")
}

async fn call_custom_validation(req: http::Request<BoxBody>) -> http::Response<BoxBody> {
    let config = custom_validation_exception_example::CustomValidationExampleConfig::builder().build();
    let app = custom_validation_exception_example::CustomValidationExample::builder::<BoxBody, _, _, _>(config)
        .test_operation(custom_validation_handler)
        .build_unchecked();
    app.oneshot(req).await.expect("service call must not fail")
}

const JSON11_CT: &str = "application/x-amz-json-1.1";
const JSON10_CT: &str = "application/x-amz-json-1.0";
const VALID_CONB: &str = r#"{"nice":"n","int":1}"#;

// ---------------------------------------------------------------------------
// B2 — awsJson 1.0 / 1.1 `__type` value forms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b2_awsjson11_modeled_error_into_response() {
    let err = json_rpc11::error::GreetingWithErrorsError::InvalidGreeting(
        json_rpc11::error::InvalidGreeting {
            message: Some("Hi".to_owned()),
        },
    );
    let response = IntoResponse::<
        aws_smithy_http_server::protocol::aws_json_11::AwsJson1_1,
    >::into_response(err);
    dump_response("B2 awsJson1.1 modeled error (InvalidGreeting) via IntoResponse", response).await;
}

#[tokio::test]
async fn b2_awsjson10_modeled_error_into_response() {
    let err = json_rpc10::error::GreetingWithErrorsError::InvalidGreeting(
        json_rpc10::error::InvalidGreeting {
            message: Some("Hi".to_owned()),
        },
    );
    let response = IntoResponse::<
        aws_smithy_http_server::protocol::aws_json_10::AwsJson1_0,
    >::into_response(err);
    dump_response("B2 awsJson1.0 modeled error (InvalidGreeting) via IntoResponse", response).await;
}

#[tokio::test]
async fn b2_awsjson11_modeled_error_through_router() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON11_CT), ("x-amz-target", "JsonProtocol.GreetingWithErrors")],
        b"{}",
    );
    let response = call_json11(req).await;
    dump_response("B2 awsJson1.1 modeled error (InvalidGreeting) through router", response).await;
}

#[tokio::test]
async fn b2_awsjson10_modeled_error_through_router() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON10_CT), ("x-amz-target", "JsonRpc10.GreetingWithErrors")],
        b"{}",
    );
    let response = call_json10(req).await;
    dump_response("B2 awsJson1.0 modeled error (InvalidGreeting) through router", response).await;
}

#[tokio::test]
async fn b2_awsjson11_unknown_operation() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON11_CT), ("x-amz-target", "JsonProtocol.DoesNotExist")],
        b"{}",
    );
    let response = call_json11(req).await;
    dump_response("B2/B6 awsJson1.1 unknown operation in x-amz-target", response).await;
}

#[tokio::test]
async fn b2_awsjson10_unknown_operation() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON10_CT), ("x-amz-target", "JsonRpc10.DoesNotExist")],
        b"{}",
    );
    let response = call_json10(req).await;
    dump_response("B2/B6 awsJson1.0 unknown operation in x-amz-target", response).await;
}

#[tokio::test]
async fn b2_awsjson11_malformed_body() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON11_CT), ("x-amz-target", "JsonProtocol.GreetingWithErrors")],
        b"{",
    );
    let response = call_json11(req).await;
    dump_response("B2 awsJson1.1 malformed/unparseable body", response).await;
}

#[tokio::test]
async fn b2_awsjson10_malformed_body() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON10_CT), ("x-amz-target", "JsonRpc10.GreetingWithErrors")],
        b"{",
    );
    let response = call_json10(req).await;
    dump_response("B2 awsJson1.0 malformed/unparseable body", response).await;
}

// NOTE: GreetingWithErrors has an EMPTY input structure; for empty inputs the generated
// deserializer never parses the body, so the "malformed body" capture above exercises the
// route but not the JSON parser. This one targets SimpleScalarProperties (has body members).
#[tokio::test]
async fn b2_awsjson11_malformed_body_bodyful_operation() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON11_CT), ("x-amz-target", "JsonProtocol.SimpleScalarProperties")],
        b"{",
    );
    let response = call_json11(req).await;
    dump_response("B2 awsJson1.1 malformed body (SimpleScalarProperties, body-ful input)", response).await;
}

#[tokio::test]
async fn b2_awsjson11_type_mismatch_body_bodyful_operation() {
    let req = request(
        "POST",
        "/",
        &[("content-type", JSON11_CT), ("x-amz-target", "JsonProtocol.SimpleScalarProperties")],
        br#"{"floatValue":"not-a-number-and-not-a-string-field"#,
    );
    let response = call_json11(req).await;
    dump_response("B2 awsJson1.1 truncated/mismatched body (SimpleScalarProperties)", response).await;
}

// ---------------------------------------------------------------------------
// B3 — rpcv2Cbor modeled error `__type` form
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b3_rpcv2cbor_modeled_error_into_response() {
    let err = rpcv2cbor::error::GreetingWithErrorsError::InvalidGreeting(
        rpcv2cbor::error::InvalidGreeting {
            message: Some("Hi".to_owned()),
        },
    );
    let response = IntoResponse::<
        aws_smithy_http_server::protocol::rpc_v2_cbor::RpcV2Cbor,
    >::into_response(err);
    let body = dump_response("B3 rpcv2Cbor modeled error (InvalidGreeting) via IntoResponse", response).await;
    dump_cbor_diag("B3 rpcv2Cbor modeled error", &body);
}

#[tokio::test]
async fn b3_rpcv2cbor_modeled_error_through_router() {
    let req = request(
        "POST",
        "/service/RpcV2Protocol/operation/GreetingWithErrors",
        &[
            ("content-type", "application/cbor"),
            ("smithy-protocol", "rpc-v2-cbor"),
            ("accept", "application/cbor"),
        ],
        &[0xA0], // empty CBOR map
    );
    let response = call_cbor(req).await;
    let body = dump_response("B3 rpcv2Cbor modeled error (InvalidGreeting) through router", response).await;
    dump_cbor_diag("B3 rpcv2Cbor modeled error through router", &body);
}

// ---------------------------------------------------------------------------
// B5 — custom @validationException shape wire form
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b5_custom_validation_constraint_violations() {
    let req = request(
        "POST",
        "/test",
        &[("content-type", "application/json")],
        br#"{"name":"this-name-is-way-too-long","age":500}"#,
    );
    let response = call_custom_validation(req).await;
    dump_response("B5 custom @validationException: @length + @range violations", response).await;
}

#[tokio::test]
async fn b5_custom_validation_length_only() {
    let req = request(
        "POST",
        "/test",
        &[("content-type", "application/json")],
        br#"{"name":"this-name-is-way-too-long"}"#,
    );
    let response = call_custom_validation(req).await;
    dump_response("B5 custom @validationException: @length violation only", response).await;
}

#[tokio::test]
async fn b5_custom_validation_missing_required() {
    let req = request(
        "POST",
        "/test",
        &[("content-type", "application/json")],
        br#"{"age":5}"#,
    );
    let response = call_custom_validation(req).await;
    dump_response("B5 custom @validationException: missing @required member `name`", response).await;
}

#[tokio::test]
async fn b5_smithy_framework_validation_exception_for_comparison() {
    // Normal smithy.framework#ValidationException from the constraints crate (restJson1).
    let body = format!(r#"{{"conA":{{"conB":{VALID_CONB},"lengthString":"a"}}}}"#);
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "application/json")],
        body.as_bytes(),
    );
    let response = call_constraints(req).await;
    dump_response("B5 comparison: smithy.framework#ValidationException (constraints crate)", response).await;
}

// ---------------------------------------------------------------------------
// B6 — synthetic errors per protocol
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b6_restjson1_unknown_route() {
    let req = request("POST", "/this-route-does-not-exist", &[("content-type", "application/json")], b"{}");
    let response = call_constraints(req).await;
    dump_response("B6 restJson1 unknown route", response).await;
}

#[tokio::test]
async fn b6_restjson1_wrong_content_type() {
    let body = format!(r#"{{"conA":{{"conB":{VALID_CONB}}}}}"#);
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "text/plain")],
        body.as_bytes(),
    );
    let response = call_constraints(req).await;
    dump_response("B6 restJson1 wrong content-type on valid route", response).await;
}

#[tokio::test]
async fn b6_restjson1_bad_accept() {
    let body = format!(r#"{{"conA":{{"conB":{VALID_CONB}}}}}"#);
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "application/json"), ("accept", "application/xml")],
        body.as_bytes(),
    );
    let response = call_constraints(req).await;
    dump_response("B6 restJson1 unacceptable Accept header", response).await;
}

#[tokio::test]
async fn b6_awsjson11_wrong_content_type() {
    let req = request(
        "POST",
        "/",
        &[("content-type", "text/plain"), ("x-amz-target", "JsonProtocol.GreetingWithErrors")],
        b"{}",
    );
    let response = call_json11(req).await;
    dump_response("B6 awsJson1.1 wrong content-type on valid route", response).await;
}

#[tokio::test]
async fn b6_awsjson11_bad_accept() {
    let req = request(
        "POST",
        "/",
        &[
            ("content-type", JSON11_CT),
            ("x-amz-target", "JsonProtocol.GreetingWithErrors"),
            ("accept", "application/xml"),
        ],
        b"{}",
    );
    let response = call_json11(req).await;
    dump_response("B6 awsJson1.1 unacceptable Accept header", response).await;
}

#[tokio::test]
async fn b6_rpcv2cbor_unknown_operation() {
    let req = request(
        "POST",
        "/service/RpcV2Protocol/operation/DoesNotExist",
        &[
            ("content-type", "application/cbor"),
            ("smithy-protocol", "rpc-v2-cbor"),
            ("accept", "application/cbor"),
        ],
        &[0xA0],
    );
    let response = call_cbor(req).await;
    let body = dump_response("B6 rpcv2Cbor unknown operation", response).await;
    dump_cbor_diag("B6 rpcv2Cbor unknown operation", &body);
}

#[tokio::test]
async fn b6_rpcv2cbor_wrong_content_type() {
    let req = request(
        "POST",
        "/service/RpcV2Protocol/operation/GreetingWithErrors",
        &[
            ("content-type", "application/json"),
            ("smithy-protocol", "rpc-v2-cbor"),
            ("accept", "application/cbor"),
        ],
        b"{}",
    );
    let response = call_cbor(req).await;
    let body = dump_response("B6 rpcv2Cbor wrong content-type on valid route", response).await;
    dump_cbor_diag("B6 rpcv2Cbor wrong content-type", &body);
}

#[tokio::test]
async fn b6_rpcv2cbor_bad_accept() {
    let req = request(
        "POST",
        "/service/RpcV2Protocol/operation/GreetingWithErrors",
        &[
            ("content-type", "application/cbor"),
            ("smithy-protocol", "rpc-v2-cbor"),
            ("accept", "text/plain"),
        ],
        &[0xA0],
    );
    let response = call_cbor(req).await;
    let body = dump_response("B6 rpcv2Cbor unacceptable Accept header", response).await;
    dump_cbor_diag("B6 rpcv2Cbor bad accept", &body);
}

#[tokio::test]
async fn b6_rpcv2cbor_missing_smithy_protocol_header() {
    let req = request(
        "POST",
        "/service/RpcV2Protocol/operation/GreetingWithErrors",
        &[("content-type", "application/cbor"), ("accept", "application/cbor")],
        &[0xA0],
    );
    let response = call_cbor(req).await;
    let body = dump_response("B6 rpcv2Cbor missing smithy-protocol header", response).await;
    dump_cbor_diag("B6 rpcv2Cbor missing smithy-protocol header", &body);
}

// GreetingWithErrors (empty input) skips the content-type check entirely; this targets a
// body-ful operation so the content-type enforcement actually runs.
#[tokio::test]
async fn b6_awsjson11_wrong_content_type_bodyful_operation() {
    let req = request(
        "POST",
        "/",
        &[("content-type", "text/plain"), ("x-amz-target", "JsonProtocol.SimpleScalarProperties")],
        b"{}",
    );
    let response = call_json11(req).await;
    dump_response("B6 awsJson1.1 wrong content-type (SimpleScalarProperties, body-ful input)", response).await;
}

#[tokio::test]
async fn b6_rpcv2cbor_wrong_content_type_bodyful_operation() {
    let req = request(
        "POST",
        "/service/RpcV2CborService/operation/SimpleStructOperation",
        &[
            ("content-type", "application/json"),
            ("smithy-protocol", "rpc-v2-cbor"),
            ("accept", "application/cbor"),
        ],
        b"{}",
    );
    let response = call_cbor_extras(req).await;
    let body = dump_response(
        "B6 rpcv2Cbor wrong content-type (SimpleStructOperation, body-ful input)",
        response,
    )
    .await;
    dump_cbor_diag("B6 rpcv2Cbor wrong content-type body-ful", &body);
}

// ---------------------------------------------------------------------------
// C1 + C2 — fail-fast vs aggregation; exact message templates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn c1_two_violations_on_different_members() {
    let body = format!(
        r#"{{"conA":{{"conB":{VALID_CONB},"lengthString":"a","rangeInteger":999}}}}"#
    );
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "application/json")],
        body.as_bytes(),
    );
    let response = call_constraints(req).await;
    let body = dump_response("C1 restJson1 TWO constraint violations in one request", response).await;
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
        let n = json
            .get("fieldList")
            .and_then(|f| f.as_array())
            .map(|a| a.len());
        println!("C1 fieldList entry count: {n:?}");
    }
}

#[tokio::test]
async fn c2_single_length_violation() {
    let body = format!(r#"{{"conA":{{"conB":{VALID_CONB},"lengthString":"a"}}}}"#);
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "application/json")],
        body.as_bytes(),
    );
    let response = call_constraints(req).await;
    dump_response("C2 restJson1 single @length violation", response).await;
}

#[tokio::test]
async fn c2_single_range_violation() {
    let body = format!(r#"{{"conA":{{"conB":{VALID_CONB},"rangeInteger":999}}}}"#);
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "application/json")],
        body.as_bytes(),
    );
    let response = call_constraints(req).await;
    dump_response("C2 restJson1 single @range violation", response).await;
}

#[tokio::test]
async fn c2_single_pattern_violation() {
    // NOTE: `fixedValue*` members are primitive (non-Option) with implicit default 0, but their
    // @range is (10..10)/(69..69); omitting them makes the generated builder PANIC at request time
    // ("this check should have failed at generation time"). Supply satisfying values so only the
    // @pattern violation remains. See side-finding in the report.
    let body = format!(
        r#"{{"conA":{{"conB":{VALID_CONB},"fixedValueInteger":69,"fixedValueShort":10,"fixedValueLong":10,"fixedValueByte":10,"patternString":"zzzzz"}}}}"#
    );
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "application/json")],
        body.as_bytes(),
    );
    let response = call_constraints(req).await;
    dump_response("C2 restJson1 single @pattern violation", response).await;
}

// ---------------------------------------------------------------------------
// C6 — missing @required member per protocol
// ---------------------------------------------------------------------------

#[tokio::test]
async fn c6_restjson1_missing_required() {
    let req = request(
        "POST",
        "/constrained-shapes-operation",
        &[("content-type", "application/json")],
        b"{}",
    );
    let response = call_constraints(req).await;
    dump_response("C6 restJson1 missing @required member (conA)", response).await;
}

#[tokio::test]
async fn c6_awsjson11_missing_required() {
    let req = request(
        "POST",
        "/",
        &[
            ("content-type", JSON11_CT),
            ("x-amz-target", "JsonProtocol.EndpointWithHostLabelOperation"),
        ],
        b"{}",
    );
    let response = call_json11(req).await;
    dump_response("C6 awsJson1.1 missing @required member (label)", response).await;
}

#[tokio::test]
async fn c6_awsjson10_missing_required() {
    let req = request(
        "POST",
        "/",
        &[
            ("content-type", JSON10_CT),
            ("x-amz-target", "JsonRpc10.EndpointWithHostLabelOperation"),
        ],
        b"{}",
    );
    let response = call_json10(req).await;
    dump_response("C6 awsJson1.0 missing @required member (label)", response).await;
}

#[tokio::test]
async fn c6_rpcv2cbor_missing_required() {
    let req = request(
        "POST",
        "/service/RpcV2CborService/operation/SimpleStructOperation",
        &[
            ("content-type", "application/cbor"),
            ("smithy-protocol", "rpc-v2-cbor"),
            ("accept", "application/cbor"),
        ],
        &[0xA0], // empty CBOR map: all @required members missing
    );
    let response = call_cbor_extras(req).await;
    let body = dump_response("C6 rpcv2Cbor missing @required members (SimpleStructOperation)", response).await;
    dump_cbor_diag("C6 rpcv2Cbor missing required", &body);
}

// ---------------------------------------------------------------------------
// SIDE-FINDING — request-time PANIC (not a 400) for omitted primitive members whose
// implicit default (0) violates their own @range constraint.
// ---------------------------------------------------------------------------

/// `ConA.fixedValueInteger` targets `@range(min: 69, max: 69) integer FixedValueInteger`
/// (primitive shape => implicit default 0, member generated as non-`Option` with a
/// `0.try_into().expect(...)` fallback). A request that omits it and contains no *earlier*
/// constraint violation makes the generated builder PANIC while deserializing, instead of
/// returning a 400 ValidationException. Under a real hyper server this tears down the
/// worker/connection (or 500s with a catch-panic layer) — it never produces a modeled error.
#[tokio::test]
async fn side_finding_default_violating_range_panics_at_request_time() {
    let body = format!(r#"{{"conA":{{"conB":{VALID_CONB}}}}}"#);
    let handle = tokio::spawn(async move {
        let req = request(
            "POST",
            "/constrained-shapes-operation",
            &[("content-type", "application/json")],
            body.as_bytes(),
        );
        call_constraints(req).await
    });
    let result = handle.await;
    match result {
        Err(join_err) if join_err.is_panic() => {
            let panic_payload = join_err.into_panic();
            let msg = panic_payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic_payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "<non-string panic payload>".to_owned());
            println!("\n===== CAPTURE: SIDE-FINDING constraints crate request-time PANIC =====");
            println!("REQUEST: POST /constrained-shapes-operation, body {{\"conA\":{{\"conB\":{VALID_CONB}}}}}");
            println!("RESULT: generated service PANICKED during deserialization (no HTTP response)");
            println!("PANIC MESSAGE: {msg}");
            println!("===== END: SIDE-FINDING =====");
        }
        Err(other) => panic!("unexpected join error: {other:?}"),
        Ok(response) => {
            let body = dump_response(
                "SIDE-FINDING (no panic?): valid-minimal request to constrained-shapes-operation",
                response,
            )
            .await;
            let _ = body;
            panic!("expected the generated service to panic; it returned a response instead");
        }
    }
}

// ---------------------------------------------------------------------------
// C3 — @length counts code points or bytes?
// ---------------------------------------------------------------------------

#[test]
fn c3_length_code_points_vs_bytes() {
    use constraints::model::LengthString; // @length(min: 2, max: 69)

    // 1 code point, 4 UTF-8 bytes: passes min=2 iff bytes are counted.
    let one_rocket = LengthString::try_from("\u{1F680}".to_owned());
    println!("C3: 1 rocket (1 code point / 4 bytes) => {one_rocket:?}");

    // 2 code points, 8 UTF-8 bytes: passes either way.
    let two_rockets = LengthString::try_from("\u{1F680}\u{1F680}".to_owned());
    println!("C3: 2 rockets (2 code points / 8 bytes) => {:?}", two_rockets.is_ok());

    // 69 code points, 276 UTF-8 bytes: passes max=69 iff code points are counted.
    let sixty_nine = LengthString::try_from("\u{1F680}".repeat(69));
    println!("C3: 69 rockets (69 code points / 276 bytes) => {:?}", sixty_nine.is_ok());

    // 70 code points: fails either way.
    let seventy = LengthString::try_from("\u{1F680}".repeat(70));
    println!("C3: 70 rockets (70 code points / 280 bytes) => {seventy:?}");

    // Conclusion assertions: @length counts Unicode code points (chars), not bytes.
    assert!(one_rocket.is_err(), "1 code point must violate min=2 => counts code points");
    assert!(two_rockets.is_ok());
    assert!(sixty_nine.is_ok(), "69 code points (276 bytes) must satisfy max=69 => counts code points");
    assert!(seventy.is_err());
    println!("C3 CONCLUSION: @length on strings counts Unicode CODE POINTS, not bytes.");
}
