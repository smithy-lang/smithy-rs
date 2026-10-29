/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Exercises rejection logging through the actual schema upgrade services. Request failures
//! must retain their Display diagnostics at DEBUG, and missing handler extensions at ERROR,
//! as legacy FromRequest does, including when unredacted-logging is disabled.

use super::*;
use std::io::Write;
use std::sync::Mutex;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}

async fn capture(
    future: impl Future<Output = Result<http::Response<BoxBody>, Infallible>>,
) -> (http::Response<BoxBody>, String) {
    let writer = LogWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(writer.clone())
        .finish();
    let response = future.with_subscriber(subscriber).await.unwrap();
    let logs = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
    (response, logs)
}

fn assert_diagnostic(logs: &str, level: &str, message: &str, diagnostic: &str) {
    assert!(logs.contains(level), "{logs}");
    assert!(logs.contains(message), "{logs}");
    assert!(logs.contains(diagnostic), "{logs}");
    assert!(logs.contains("operation=test#Operation"), "{logs}");
    assert!(logs.contains("protocol="), "{logs}");
    assert!(!logs.contains("{redacted}"), "{logs}");
}

static NAME: Schema<'static> =
    Schema::new_member(shape_id!("test", "LoggingInput", "name"), ShapeType::String, "name", 0);
static LOGGING_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("test", "LoggingInput"), ShapeType::Structure, &[&NAME]);

struct LoggingInput;

impl DeserializableShape for LoggingInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        deserializer.read_struct(&LOGGING_INPUT, &mut |member, value| {
            value.read_string(member)?;
            Ok(())
        })?;
        Ok(Self)
    }
}

struct LoggingOperation;

impl OperationShape for LoggingOperation {
    const ID: crate::shape_id::ShapeId = Ordinary::ID;
    type Input = LoggingInput;
    type Output = EmptyShape;
    type Error = Infallible;
}

impl SchemaOperationShape for LoggingOperation {
    const SCHEMA: &'static OperationSchema<'static> = {
        static SCHEMA: OperationSchema<'static> =
            OperationSchema::new(shape_id!("test", "Operation"), &LOGGING_INPUT, &EMPTY, &[]);
        &SCHEMA
    };
}

#[tokio::test]
async fn malformed_cbor_logs_the_parse_diagnostic_without_reaching_the_handler() {
    let mut request = http::Request::builder()
        .header("content-type", "application/cbor")
        .body(http_body_util::Full::new(Bytes::from_static(&[0xff])))
        .unwrap();
    SelectedOperation::new(
        SharedServerProtocol::metadata_routed(crate::schema::protocol::RpcV2Cbor::default()),
        LoggingOperation::SCHEMA,
        Default::default(),
    )
    .insert_into(request.extensions_mut());
    let upgrade = DynUpgrade::<crate::protocol::rpc_v2_cbor::RpcV2Cbor, LoggingOperation, (), _> {
        inner: tower::service_fn(|_: (LoggingInput, ())| async {
            panic!("malformed input must not reach the handler");
            #[allow(unreachable_code)]
            Ok::<_, Infallible>(EmptyShape)
        }),
        _protocol: PhantomData,
        _operation: PhantomData,
        _extractors: PhantomData,
    };
    let (response, logs) = capture(upgrade.oneshot(request)).await;
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    assert_diagnostic(
        &logs,
        "DEBUG",
        "failed to deserialize request into operation's input",
        "expected map",
    );
}

static BIG_NUMBER: Schema<'static> = Schema::new_member(
    shape_id!("test", "BigNumberInput", "value"),
    ShapeType::BigDecimal,
    "value",
    0,
)
.with_http_query("value")
.with_sensitive();
static BIG_NUMBER_INPUT: Schema<'static> = Schema::new_struct(
    shape_id!("test", "BigNumberInput"),
    ShapeType::Structure,
    &[&BIG_NUMBER],
)
.with_http(aws_smithy_schema::traits::HttpTrait::new("GET", "/", Some(200)));

struct BigNumberInput;

impl DeserializableShape for BigNumberInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        deserializer.read_struct(&BIG_NUMBER_INPUT, &mut |member, value| {
            value.read_big_decimal(member)?;
            Ok(())
        })?;
        Ok(Self)
    }
}

struct BigNumberOperation;

impl OperationShape for BigNumberOperation {
    const ID: crate::shape_id::ShapeId = Ordinary::ID;
    type Input = BigNumberInput;
    type Output = EmptyShape;
    type Error = Infallible;
}

impl SchemaOperationShape for BigNumberOperation {
    const SCHEMA: &'static OperationSchema<'static> = {
        static SCHEMA: OperationSchema<'static> =
            OperationSchema::new(shape_id!("test", "Operation"), &BIG_NUMBER_INPUT, &EMPTY, &[]);
        &SCHEMA
    };
}

#[tokio::test]
async fn big_number_rejections_do_not_include_request_values_in_logs_or_responses() {
    const INVALID_VALUE: &str = "private-account-number-invalid";
    let mut request = http::Request::builder()
        .uri(format!("/?value={INVALID_VALUE}"))
        .body(http_body_util::Empty::<Bytes>::new())
        .unwrap();
    SelectedOperation::new(
        SharedServerProtocol::metadata_routed(crate::schema::protocol::RestJson1::default()),
        BigNumberOperation::SCHEMA,
        Default::default(),
    )
    .insert_into(request.extensions_mut());
    let upgrade = DynUpgrade::<crate::protocol::rest_json_1::RestJson1, BigNumberOperation, (), _> {
        inner: tower::service_fn(|_: (BigNumberInput, ())| async {
            panic!("malformed input must not reach the handler");
            #[allow(unreachable_code)]
            Ok::<_, Infallible>(EmptyShape)
        }),
        _protocol: PhantomData,
        _operation: PhantomData,
        _extractors: PhantomData,
    };
    let (response, logs) = capture(upgrade.oneshot(request)).await;
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    assert_diagnostic(
        &logs,
        "DEBUG",
        "failed to deserialize request into operation's input",
        "invalid big decimal",
    );
    assert!(!logs.contains(INVALID_VALUE), "{logs}");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&body).contains(INVALID_VALUE));
}

struct RejectedStreaming;

impl OperationShape for RejectedStreaming {
    const ID: crate::shape_id::ShapeId = Ordinary::ID;
    type Input = ();
    type Output = ();
    type Error = Infallible;
}

impl SchemaOperationShape for RejectedStreaming {
    const SCHEMA: &'static OperationSchema<'static> = Ordinary::SCHEMA;
}

impl StreamingOperationShape for RejectedStreaming {
    fn deserialize_streaming_input(
        _: aws_smithy_runtime_api::http::Request<crate::body::RequestBody>,
        _: SharedServerProtocol,
    ) -> crate::operation::StreamingInputFuture<()> {
        Box::pin(async {
            Err(aws_smithy_schema::serde::SerdeError::custom("initial request frame is truncated").into())
        })
    }

    fn serialize_streaming_output(_: (), _: &SharedServerProtocol) -> http::Response<BoxBody> {
        panic!("rejected input must not produce an output")
    }
}

#[tokio::test]
async fn streaming_input_failure_logs_its_diagnostic_without_reaching_the_handler() {
    let mut request = http::Request::new(http_body_util::Empty::<Bytes>::new());
    SelectedOperation::new(
        SharedServerProtocol::metadata_routed(crate::schema::protocol::RpcV2Cbor::default()),
        RejectedStreaming::SCHEMA,
        Default::default(),
    )
    .insert_into(request.extensions_mut());
    let upgrade = DynStreamingUpgrade::<crate::protocol::rpc_v2_cbor::RpcV2Cbor, RejectedStreaming, (), _> {
        inner: tower::service_fn(|_: ((), ())| async {
            panic!("rejected streaming input must not reach the handler");
            #[allow(unreachable_code)]
            Ok::<_, Infallible>(())
        }),
        _protocol: PhantomData,
        _operation: PhantomData,
        _extractors: PhantomData,
    };
    let (response, logs) = capture(upgrade.oneshot(request)).await;
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    assert_diagnostic(
        &logs,
        "DEBUG",
        "failed to deserialize request into operation's input",
        "initial request frame is truncated",
    );
}

#[tokio::test]
async fn missing_extensions_log_the_extractor_diagnostic_for_both_upgrade_services() {
    use crate::Extension;

    for streaming in [false, true] {
        let mut request = http::Request::new(http_body_util::Empty::<Bytes>::new());
        SelectedOperation::new(
            SharedServerProtocol::metadata_routed(crate::schema::protocol::RpcV2Cbor::default()),
            Ordinary::SCHEMA,
            Default::default(),
        )
        .insert_into(request.extensions_mut());
        let (response, logs) = if streaming {
            capture(
                DynStreamingUpgrade::<crate::protocol::rpc_v2_cbor::RpcV2Cbor, Ordinary, Extension<String>, _> {
                    inner: tower::service_fn(|_: ((), Extension<String>)| async {
                        panic!("missing extension must not reach the handler");
                        #[allow(unreachable_code)]
                        Ok::<_, Infallible>(())
                    }),
                    _protocol: PhantomData,
                    _operation: PhantomData,
                    _extractors: PhantomData,
                }
                .oneshot(request),
            )
            .await
        } else {
            capture(
                DynUpgrade::<crate::protocol::rpc_v2_cbor::RpcV2Cbor, NonStreaming, Extension<String>, _> {
                    inner: tower::service_fn(|_: (EmptyShape, Extension<String>)| async {
                        panic!("missing extension must not reach the handler");
                        #[allow(unreachable_code)]
                        Ok::<_, Infallible>(EmptyShape)
                    }),
                    _protocol: PhantomData,
                    _operation: PhantomData,
                    _extractors: PhantomData,
                }
                .oneshot(request),
            )
            .await
        };
        assert_eq!(response.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_diagnostic(
            &logs,
            "ERROR",
            "additional parameter for the handler function could not be constructed",
            "the `Extension` is not present in the `http::Request`",
        );
    }
}
