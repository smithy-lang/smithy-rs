/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use super::*;
use crate::schema::protocol::{RestJson1Protocol, RpcV2CborProtocol};
use crate::schema::{OperationSchema, SharedServerProtocol};
use aws_smithy_schema::{shape_id, ShapeType};
use std::sync::atomic::{AtomicUsize, Ordering};

static DATA: Schema =
    Schema::new_member(shape_id!("test", "Input", "fahad"), ShapeType::Blob, "fahad", 0).with_streaming();
static TITLE: Schema = Schema::new_member(shape_id!("test", "Input", "title"), ShapeType::String, "title", 1);
static META: Schema = Schema::new_member(shape_id!("test", "Input", "meta"), ShapeType::Structure, "meta", 2);
static MAP: Schema = Schema::new_member(shape_id!("test", "Nested", "map"), ShapeType::Map, "map", 0);
static NESTED: Schema = Schema::new_struct(shape_id!("test", "Nested"), ShapeType::Structure, &[&MAP]);
static INPUT: Schema = Schema::new_struct(
    shape_id!("test", "Input"),
    ShapeType::Structure,
    &[&DATA, &TITLE, &META],
);
static OP: OperationSchema = OperationSchema::new(shape_id!("test", "Upload"), &INPUT, &INPUT, &[]);

#[derive(Default, Debug, PartialEq)]
struct Fields {
    title: Option<String>,
    meta: Option<Vec<Option<String>>>,
}

fn request(body: RequestBody, config: RequestBodyCollectionConfig) -> Request<RequestBody> {
    let mut request = http::Request::builder()
        .header("content-type", "application/cbor")
        .body(body)
        .unwrap();
    request.extensions_mut().insert(SelectedOperation::new(
        SharedServerProtocol::metadata_routed(RpcV2CborProtocol::default()),
        &OP,
        config,
    ));
    Request::try_from(request).unwrap()
}

async fn prepare(
    body: RequestBody,
    config: RequestBodyCollectionConfig,
) -> Result<StreamingInput<Fields>, DeserializeError> {
    let decoder = StreamingRequestDecoder::cbor(
        &DATA,
        request(body, config),
        Arc::new(aws_smithy_cbor::codec::CborCodec::new(
            aws_smithy_cbor::codec::CborCodecSettings::default().enforce_strictness(true),
        )),
    )?;
    StreamingInput::prepare(
        decoder,
        Fields::default(),
        |fields, deserializer| {
            deserializer.read_struct(&INPUT, &mut |member, deserializer| {
                match member.member_index() {
                    Some(1) => fields.title = Some(deserializer.read_string(member)?),
                    Some(2) => {
                        let mut values = Vec::new();
                        deserializer.read_struct(&NESTED, &mut |member, d| {
                            d.read_map(member, &mut |key, d| {
                                assert_eq!(key, "list");
                                d.read_list(member, &mut |d| {
                                    values.push(if d.is_null() {
                                        d.read_null()?;
                                        None
                                    } else {
                                        Some(d.read_string(&TITLE)?)
                                    });
                                    Ok(())
                                })
                            })
                        })?;
                        fields.meta = Some(values);
                    }
                    _ => {}
                }
                Ok(())
            })
        },
        |fields, _| Ok(fields),
    )
    .await
}

fn fragmented(bytes: Vec<u8>) -> RequestBody {
    RequestBody::new(http_body_util::StreamBody::new(futures_util::stream::iter(
        bytes
            .into_iter()
            .map(|byte| Ok::<_, std::io::Error>(Frame::data(Bytes::from(vec![byte])))),
    )))
}

#[tokio::test]
async fn fields_before_and_after_fragmented_blob_and_nested_values() {
    for before in [false, true] {
        let mut encoder = aws_smithy_cbor::Encoder::new(Vec::new());
        encoder.begin_map();
        if before {
            encoder.str("title").str("hello");
        }
        encoder
            .str("meta")
            .begin_map()
            .str("map")
            .begin_map()
            .str("list")
            .begin_array()
            .str("one")
            .null()
            .end()
            .end()
            .end();
        encoder.str("fahad");
        let mut bytes = encoder.into_writer();
        bytes.extend_from_slice(&[0x5f, 0x42, b'h', b'e', 0x40, 0x43, b'l', b'l', b'o', 0xff]);
        let mut encoder = aws_smithy_cbor::Encoder::new(bytes);
        if !before {
            encoder.str("title").str("hello");
        }
        encoder.end();
        let mut input = prepare(fragmented(encoder.into_writer()), Default::default())
            .await
            .unwrap();
        assert!(input.is_present());
        assert!(matches!(
            input.finish().await,
            Err(StreamingInputError::StreamNotFinished)
        ));
        let mut blob = Vec::new();
        while let Some(bytes) = input.data.next().await {
            blob.extend_from_slice(&bytes.unwrap());
        }
        assert_eq!(blob, b"hello");
        let fields = input.finish().await.unwrap();
        assert_eq!(fields.title.as_deref(), Some("hello"));
        assert!(fields.meta.is_some());
        assert!(matches!(
            input.finish().await,
            Err(StreamingInputError::AlreadyFinished)
        ));
    }
}

#[tokio::test]
async fn presence_and_empty_blob_are_distinct() {
    for (wire, present) in [
        (vec![], false),
        (vec![0xa0], false),
        (b"\xa1\x65fahad\xf6".to_vec(), false),
        (b"\xa1\x65fahad\x40".to_vec(), true),
        (b"\xbf\x65fahad\x5f\xff\xff".to_vec(), true),
    ] {
        let input = prepare(fragmented(wire), Default::default()).await.unwrap();
        assert_eq!(input.is_present(), present);
        let (_, blob) = input.into_input_and_blob().await.unwrap();
        assert_eq!(blob.map(|blob| blob.into_inner()), present.then(Vec::new));
    }
}

#[tokio::test]
async fn duplicates_and_trailing_bytes_fail_after_blob_eof() {
    for wire in [
        b"\xa2\x65fahad\x41a\x65fahad\x41b".to_vec(),
        b"\xa1\x65fahad\x41a\x00".to_vec(),
    ] {
        let mut input = prepare(fragmented(wire), Default::default()).await.unwrap();
        let mut bytes = Vec::new();
        while let Some(chunk) = input.data.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(bytes, b"a");
        assert!(input.finish().await.is_err());
        assert!(matches!(input.finish().await, Err(StreamingInputError::Failed(_))));
    }
}

#[tokio::test]
async fn collection_rejects_previously_read_bytes() {
    let mut input = prepare(fragmented(b"\xa1\x65fahad\x42ab".to_vec()), Default::default())
        .await
        .unwrap();
    assert_eq!(input.data.next().await.unwrap().unwrap(), "a");
    assert!(matches!(
        input.into_input_and_blob().await,
        Err(StreamingInputError::AlreadyRead)
    ));
}

#[tokio::test]
async fn collection_counts_wire_bytes_already_read_in_preparation() {
    let wire = b"\xa2\x65title\x65hello\x65fahad\x43abc".to_vec();
    for (limit, success) in [(wire.len(), true), (wire.len() - 1, false)] {
        let config = RequestBodyCollectionConfig::default().with_max_bytes(std::num::NonZeroUsize::new(limit));
        let input = prepare(fragmented(wire.clone()), config).await.unwrap();
        assert_eq!(input.into_input_and_blob().await.is_ok(), success);
    }
}

#[tokio::test]
async fn truncated_blob_and_source_errors_are_terminal() {
    for partial in [false, true] {
        let mut chunks = vec![Ok(Frame::data(Bytes::from_static(b"\xa1\x65fahad\x42")))];
        if partial {
            chunks.push(Ok(Frame::data(Bytes::from_static(b"a"))));
        }
        chunks.push(Err(std::io::Error::other("source failed")));
        let body = RequestBody::new(http_body_util::StreamBody::new(futures_util::stream::iter(chunks)));
        let mut input = prepare(body, Default::default()).await.unwrap();
        if partial {
            assert_eq!(input.data.next().await.unwrap().unwrap(), "a");
        }
        assert!(input.data.next().await.unwrap().is_err());
        assert!(matches!(input.finish().await, Err(StreamingInputError::Failed(_))));
    }
    let input = prepare(fragmented(b"\xa1\x65fahad\x42a".to_vec()), Default::default())
        .await
        .unwrap();
    assert!(input.into_blob().await.is_err());
}

#[tokio::test]
async fn preparation_stops_at_the_blob_and_drop_cancels_source() {
    struct Source {
        polls: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    }
    impl Drop for Source {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Body for Source {
        type Data = Bytes;
        type Error = std::io::Error;
        fn poll_frame(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            match self.polls.fetch_add(1, Ordering::SeqCst) {
                0 => Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"\xa1\x65fahad\x43"))))),
                _ => Poll::Pending,
            }
        }
    }
    let polls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let input = prepare(
        RequestBody::new(Source {
            polls: polls.clone(),
            dropped: dropped.clone(),
        }),
        Default::default(),
    )
    .await
    .unwrap();
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    drop(input);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn collection_timeout_includes_trailing_fields() {
    let body = http_body_util::StreamBody::new(futures_util::stream::unfold(false, |sent| async move {
        if sent {
            std::future::pending::<()>().await;
        }
        Some((
            Ok::<_, std::io::Error>(Frame::data(Bytes::from_static(b"\xbf\x65fahad\x40"))),
            true,
        ))
    }));
    let config = RequestBodyCollectionConfig::default().with_read_timeout(Some(std::time::Duration::from_secs(1)));
    let input = prepare(RequestBody::new(body), config).await.unwrap();
    let error = input.into_input_and_blob().await.unwrap_err();
    assert!(error.to_string().contains("timed out"));
}

#[tokio::test]
async fn rest_exposes_raw_payload_and_empty_is_present() {
    use crate::schema::ServerProtocol;
    for bytes in [Bytes::new(), Bytes::from_static(b"raw blob")] {
        let decoder = RestJson1Protocol::default()
            .deserialize_data_stream(
                &INPUT,
                &DATA,
                request(RequestBody::from(bytes.clone()), Default::default()),
            )
            .unwrap();
        let input = StreamingInput::prepare(decoder, (), |_, _| Ok(()), |(), _| Ok(()))
            .await
            .unwrap();
        assert!(input.is_present());
        assert_eq!(input.into_blob().await.unwrap().unwrap().as_ref(), bytes);
    }
}

#[tokio::test]
async fn cancellation_of_finish_preserves_partially_read_trailing_fields() {
    use futures_util::FutureExt;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<Bytes>();
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver
            .recv()
            .await
            .map(|bytes| (Ok::<_, std::io::Error>(Frame::data(bytes)), receiver))
    });
    sender.send(Bytes::from_static(b"\xbf\x65fahad\x40")).unwrap();
    let mut input = prepare(
        RequestBody::new(http_body_util::StreamBody::new(stream)),
        Default::default(),
    )
    .await
    .unwrap();
    assert!(input.data.next().await.is_none());
    sender.send(Bytes::from_static(b"\x65title\x65he")).unwrap();
    assert!(input.finish().now_or_never().is_none());
    sender.send(Bytes::from_static(b"llo\xff")).unwrap();
    drop(sender);
    assert_eq!(input.finish().await.unwrap().title.as_deref(), Some("hello"));
}

#[tokio::test]
async fn large_definite_blob_does_not_count_toward_ordinary_field_buffer_limit() {
    let mut bytes = b"\xb8\x01\x65fahad\x5a\x00\x01\x00\x00".to_vec();
    bytes.resize(bytes.len() + 65536, b'x');
    let config = RequestBodyCollectionConfig::default().with_max_bytes(std::num::NonZeroUsize::new(64));
    let mut input = prepare(RequestBody::from(Bytes::from(bytes)), config).await.unwrap();
    assert_eq!(input.data.next().await.unwrap().unwrap().len(), 65536);
    assert!(input.data.next().await.is_none());
    input.finish().await.unwrap();
}

#[test]
fn empty_frames_yield_with_existing_budgets_and_self_wake() {
    use std::future::Future;
    use std::task::{Wake, Waker};
    struct Wakes(AtomicUsize);
    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    for collecting in [false, true] {
        let polls = Arc::new(AtomicUsize::new(0));
        let observed = polls.clone();
        let source = http_body_util::StreamBody::new(futures_util::stream::poll_fn(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(Some(Ok::<_, std::io::Error>(Frame::data(Bytes::new()))))
        }));
        let (_, mut state) = State::new(request(RequestBody::new(source), Default::default()), None).unwrap();
        state.collecting = collecting;
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(poll_fn(|cx| state.poll_blob(cx)));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        let budget = if collecting {
            DEFAULT_FRAME_BUDGET.get()
        } else {
            DEFAULT_STREAMING_CHUNK_BUDGET.get()
        };
        assert_eq!(polls.load(Ordering::SeqCst), budget);
        assert!(polls.load(Ordering::SeqCst) > 0);
        assert!(wakes.0.load(Ordering::SeqCst) > 0);
    }
}

#[tokio::test]
async fn malformed_headers_and_chunks_do_not_hang() {
    for wire in [
        b"\xa1\x65fahad\xf9\x00\x16".as_slice(),      // float bits are not a null value
        b"\xb8".as_slice(),                           // fragmented/truncated map count
        b"\xbf\x65fahad\x5f\x5f\xff\xff".as_slice(),  // nested indefinite blob
        b"\xbf\x65fahad\x5f\x61x\xff\xff".as_slice(), // text instead of blob chunk
        b"\xbf\x65fahad\x40".as_slice(),              // missing map end
        b"\xa2\x65fahad\xf6\x65fahad\x40".as_slice(), // null still counts as a duplicate key
    ] {
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            match prepare(fragmented(wire.to_vec()), Default::default()).await {
                Ok(input) => input.into_input_and_blob().await.map(|_| ()),
                Err(error) => Err(error.into()),
            }
        })
        .await
        .expect("malformed input must terminate");
        assert!(result.is_err(), "accepted {wire:02x?}");
    }
}
