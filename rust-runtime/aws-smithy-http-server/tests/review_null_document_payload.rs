use std::collections::BTreeMap;
use aws_smithy_http_server::schema::protocol::RestJson1Protocol;
use aws_smithy_http_server::schema::ServerProtocol;
use aws_smithy_runtime_api::http::Request;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeId, ShapeType};
use aws_smithy_types::Document;
use bytes::Bytes;



// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn request(method: &str, uri: &str, headers: &[(&str, &str)], body: &[u8]) -> Request<Bytes> {
    let mut builder = http::Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, http::HeaderValue::from_str(value).unwrap());
    }
    Request::try_from(builder.body(Bytes::copy_from_slice(body)).unwrap()).unwrap()
}

/// Reads one value the way generated walkers do, rendering it as a string for comparison.
fn read_value(schema: &Schema<'_>, d: &mut dyn ShapeDeserializer) -> Result<String, SerdeError> {
    Ok(match schema.shape_type() {
        ShapeType::String => d.read_string(schema)?,
        ShapeType::Boolean => d.read_boolean(schema)?.to_string(),
        ShapeType::Integer => d.read_integer(schema)?.to_string(),
        ShapeType::Long => d.read_long(schema)?.to_string(),
        ShapeType::Double => d.read_double(schema)?.to_string(),
        ShapeType::Timestamp => d.read_timestamp(schema)?.secs().to_string(),
        ShapeType::Blob => String::from_utf8_lossy(d.read_blob(schema)?.as_ref()).into_owned(),
        ShapeType::Document => format!("{:?}", d.read_document(schema)?),
        ShapeType::List => {
            let element = schema.member().expect("list member");
            let mut values = Vec::new();
            d.read_list(schema, &mut |d| {
                values.push(read_value(element, d)?);
                Ok(())
            })?;
            format!("{values:?}")
        }
        ShapeType::Map => {
            let value = schema.member().expect("map value");
            let mut entries = BTreeMap::new();
            d.read_map(schema, &mut |key, d| {
                entries.insert(key, read_value(value, d)?);
                Ok(())
            })?;
            format!("{entries:?}")
        }
        other => panic!("unsupported shape type in harness: {other:?}"),
    })
}

/// A generated-style input walker: optional members check `is_null` first and stay unset on null.
fn walk(
    protocol: &dyn ServerProtocol,
    input: &Schema<'_>,
    request: &Request<Bytes>,
) -> Result<BTreeMap<String, String>, String> {
    let mut d = protocol.deserialize_request(input, request).map_err(|e| format!("{e:?}"))?;
    let mut out = BTreeMap::new();
    d.read_struct(input, &mut |member, d| {
        // Generated walkers ignore callbacks without a member index (unknown members).
        if member.member_index().is_none() {
            return Ok(());
        }
        if d.is_null() {
            d.read_null()?;
            return Ok(());
        }
        out.insert(member.member_name().unwrap().to_string(), read_value(member, d)?);
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(out)
}

fn got(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn member(id: &'static str, name: &'static str, kind: ShapeType, index: usize) -> Schema<'static> {
    Schema::new_member(ShapeId::from_parts(id, "test", "Input"), kind, name, index)
}

fn input<'a>(members: &'a [&'a Schema<'a>], uri: &'static str) -> Schema<'a> {
    Schema::new_struct(shape_id!("test", "Input"), ShapeType::Structure, members)
        .with_original_name("Input")
        .with_http(HttpTrait::new("POST", uri, None))
}

/// FINDING: a `null` JSON body for a document `@httpPayload` is a value: legacy
/// `JsonParserGenerator.payloadParser` returns `Some(Document::Null)`, and the runtime itself says
/// "A `null` document payload stays a value". But the deserializer handed to the member consumer
/// reports `is_null() == true`, and the generated optional-member arm
/// (`if deser.is_null() { deser.read_null()?; } else { .. }`) therefore leaves the member unset.
#[ignore = "models the pre-fix generated walker; verify against generated code (multi-protocol suite)"]
#[test]
fn document_payload_null_is_a_value() {
    let doc = member("test#Input$doc", "doc", ShapeType::Document, 0).with_http_payload();
    let members = [&doc];
    let schema = input(&members, "/d");
    let req = request("POST", "/d", &[("content-type", "application/json")], b"null");
    assert_eq!(
        walk(&RestJson1Protocol::default(), &schema, &req),
        Ok(got(&[("doc", &format!("{:?}", Document::Null))]))
    );
}
