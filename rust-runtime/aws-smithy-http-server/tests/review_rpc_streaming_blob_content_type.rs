use aws_smithy_http_server::schema::protocol::RpcV2CborProtocol;
use aws_smithy_http_server::schema::ServerProtocol;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use bytes::Bytes;

// ---------------------------------------------------------------------------
// Finding: rpcv2Cbor labels streaming *blob* responses as event streams.
// ---------------------------------------------------------------------------

static BLOB_DATA: Schema<'static> =
    Schema::new_member(shape_id!("test", "DownloadOutput", "data"), ShapeType::Blob, "data", 0).with_streaming();

static DOWNLOAD_OUTPUT: Schema<'static> =
    Schema::new_struct(shape_id!("test", "DownloadOutput"), ShapeType::Structure, &[&BLOB_DATA])
        .with_original_name("DownloadOutput");

struct DownloadOutput;

impl SerializableStruct for DownloadOutput {
    fn schema(&self) -> &Schema<'_> {
        &DOWNLOAD_OUTPUT
    }
    fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        Ok(())
    }
}

/// `RpcProtocol::serialize_streaming_response` uses the event stream media type for *every*
/// streaming output. For a streaming blob the body is raw bytes, not
/// `application/vnd.amazon.eventstream` frames; legacy rpcv2Cbor
/// (`RpcV2CborHttpBindingResolver.responseContentType`) only uses the event stream type when
/// `isOutputEventStream`, and labels other modeled outputs `application/cbor`.
#[test]
fn rpcv2_cbor_streaming_blob_output_is_not_labelled_event_stream() {
    let response = RpcV2CborProtocol::default().serialize_streaming_response(
        &DOWNLOAD_OUTPUT,
        &DownloadOutput,
        &BLOB_DATA,
        Some(aws_smithy_http_server::body::from_bytes(Bytes::from_static(b"raw blob bytes"))),
    );
    let content_type = response.headers().get(http::header::CONTENT_TYPE);
    assert_ne!(
        content_type.and_then(|v| v.to_str().ok()),
        Some("application/vnd.amazon.eventstream"),
        "raw streaming blob body labelled as an event stream (status {})",
        response.status()
    );
}
