
use aws_smithy_http_server::schema::protocol::{
    AwsJson1_0Protocol, RpcV2CborProtocol,
};
use aws_smithy_http_server::schema::ServerProtocol;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use aws_smithy_types::Blob;
use http_body_util::BodyExt;

type Snapshot = (http::StatusCode, http::HeaderMap, bytes::Bytes);

fn snapshot(response: aws_smithy_http_server::response::Response) -> Snapshot {
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let (parts, body) = response.into_parts();
    let bytes = rt.block_on(body.collect()).unwrap().to_bytes();
    (parts.status, parts.headers, bytes)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

static NESTED_A: Schema = Schema::new_member(shape_id!("test", "Nested", "a"), ShapeType::String, "a", 0);

static NESTED: Schema = Schema::new_struct(shape_id!("test", "Nested"), ShapeType::Structure, &[&NESTED_A]);

static P_STRUCT: Schema = Schema::new_member(shape_id!("test", "StructPayload", "p"), ShapeType::Structure, "p", 0)
    .with_http_payload()
    .with_xml_name("Renamed");

static P_HDR: Schema = Schema::new_member(shape_id!("test", "StructPayload", "h"), ShapeType::String, "h", 1)
    .with_http_header("x-h");

static STRUCT_PAYLOAD: Schema =
    Schema::new_struct(shape_id!("test", "StructPayload"), ShapeType::Structure, &[&P_STRUCT, &P_HDR])
        .with_original_name("StructPayload");

struct NestedValue;

impl SerializableStruct for NestedValue {
    fn schema(&self) -> &Schema<'_> {
        &NESTED
    }
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&NESTED_A, "x")
    }
}

/// `set` = whether the payload member is set; header always set for STRUCT_PAYLOAD.
struct Payload(&'static Schema<'static>, bool);

impl SerializableStruct for Payload {
    fn schema(&self) -> &Schema<'_> {
        self.0
    }
    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        let member = self.0.members()[0];
        if std::ptr::eq(self.0, &STRUCT_PAYLOAD) {
            s.write_string(&P_HDR, "hv")?;
        }
        if !self.1 {
            return Ok(());
        }
        match member.shape_type() {
            ShapeType::Blob => s.write_blob(member, Blob::new(b"\x00\x01".to_vec())),
            // Generated `serialize_members` passes the payload TARGET's schema for struct/union
            // `@httpPayload` members (ServerSchemaGenerator.writeMethodForShape).
            ShapeType::Structure => s.write_struct(&NESTED, &NestedValue),
            ShapeType::Union => s.write_struct(&NESTED, &NestedValue),
            ShapeType::Document => {
                let mut map = std::collections::HashMap::new();
                map.insert("k".to_string(), aws_smithy_types::Document::from("v"));
                s.write_document(member, &aws_smithy_types::Document::Object(map))
            }
            _ => unreachable!(),
        }
    }
}

#[ignore = "OPEN review finding: RPC codecs emit an unkeyed nested struct for @httpPayload struct members (invalid JSON)"]
#[test]
fn rpc_protocols_serialize_struct_http_payload_member_as_a_keyed_body_member() {
    // Same generated value (payload member written with the TARGET schema, as codegen does)
    // served over the RPC protocols, which ignore @httpPayload: legacy writes `{"p":{"a":"x"}}`.
    let (_, _, b) = snapshot(AwsJson1_0Protocol::default().serialize_response(&STRUCT_PAYLOAD, &Payload(&STRUCT_PAYLOAD, true)));
    let body = std::str::from_utf8(&b).unwrap().to_string();
    assert!(body.contains(r#""p":{"a":"x"}"#), "awsJson1_0 body: {body}");
}

#[ignore = "OPEN review finding: RPC codecs emit an unkeyed nested struct for @httpPayload struct members (invalid JSON)"]
#[test]
fn rpc_v2_cbor_serializes_struct_http_payload_member_as_a_keyed_body_member() {
    let (_, _, b) = snapshot(RpcV2CborProtocol::default().serialize_response(&STRUCT_PAYLOAD, &Payload(&STRUCT_PAYLOAD, true)));
    // CBOR text key "p" = 0x61 0x70 followed by a map.
    assert!(contains(&b, &[0x61, b'p']), "rpcv2Cbor body: {b:02x?}");
}
