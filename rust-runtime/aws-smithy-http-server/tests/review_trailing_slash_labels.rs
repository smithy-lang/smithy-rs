use std::collections::BTreeMap;
use std::convert::Infallible;
use aws_smithy_http_server::body::{to_boxed, Body, BoxBody};
use aws_smithy_http_server::routing::SyncRoute;
use aws_smithy_http_server::schema::protocol::RestJson1Protocol;
use aws_smithy_http_server::schema::routing::MultiProtocolRoutingService;
use aws_smithy_http_server::schema::{OperationSchema, SelectedOperation, ServerProtocol, ServiceSchema};
use aws_smithy_runtime_api::http::Request;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeId, ShapeType};
use bytes::Bytes;
use http_body_util::BodyExt;
use tower::ServiceExt;

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

static TRAILING_NAME: Schema<'static> =
    Schema::new_member(shape_id!("test", "PetInput", "name"), ShapeType::String, "name", 0).with_http_label();

static TRAILING_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("test", "PetInput"), ShapeType::Structure, &[&TRAILING_NAME])
        .with_original_name("PetInput")
        .with_http(HttpTrait::new("GET", "/pets/{name}/", None));

static UNIT: Schema<'static> = Schema::new(shape_id!("smithy.api", "Unit"), ShapeType::Structure);

static GET_PET: OperationSchema<'static> =
    OperationSchema::new(shape_id!("test", "GetPet"), &TRAILING_INPUT, &UNIT, &[]);

static TRAILING_OPS: &[&OperationSchema<'static>] = &[&GET_PET];

static REST_JSON: &[ShapeId<'static>] = &[shape_id!("aws.protocols", "restJson1")];

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

/// FINDING: Smithy accepts URI patterns with a trailing slash (`UriPattern.parse` drops the
/// trailing empty segment), the REST router ignores it (`rest_router` filters empty segments),
/// and the legacy nom label parser is built from `UriPattern.segments` so it also ignores it.
/// `extract_labels` instead keeps the empty template segment, so every routed request fails the
/// label re-match with "request URI does not match `@http` URI pattern" (400).
#[test]
fn label_extraction_ignores_trailing_slash_in_uri_pattern() {
    let name = member("test#Input$name", "name", ShapeType::String, 0).with_http_label();
    let members = [&name];
    let schema = input(&members, "/pets/{name}/");
    let req = request("POST", "/pets/rex", &[], b"");
    assert_eq!(walk(&RestJson1Protocol::default(), &schema, &req), Ok(got(&[("name", "rex")])));
}

/// FINDING (same root cause) for a greedy label followed by a trailing slash.
#[test]
fn greedy_label_extraction_ignores_trailing_slash_in_uri_pattern() {
    let key = member("test#Input$key", "key", ShapeType::String, 0).with_http_label();
    let members = [&key];
    let schema = input(&members, "/data/{key+}/");
    let req = request("POST", "/data/a/b", &[], b"");
    assert_eq!(walk(&RestJson1Protocol::default(), &schema, &req), Ok(got(&[("key", "a/b")])));
}

/// Evidence for the finding above: the router *does* select the operation for `/pets/rex`
/// (and rejects `/pets/rex/`), so the 400 from label extraction is reached for real traffic.
#[tokio::test]
async fn evidence_router_selects_trailing_slash_pattern_without_the_slash() {
    let service = Box::leak(Box::new(ServiceSchema::new(
        shape_id!("test", "Service"),
        None,
        REST_JSON,
        TRAILING_OPS,
    )));
    let app = MultiProtocolRoutingService::builder(service)
        .operation_handler_bindings(TRAILING_OPS.iter().map(|op| {
            (
                *op,
                SyncRoute::new(tower::service_fn(|request: http::Request<Body>| async move {
                    let selected = SelectedOperation::get_from_request(&request).expect("selection");
                    Ok::<_, Infallible>(http::Response::new(to_boxed(selected.operation().shape_id().to_string())))
                })),
            )
        }))
        .build()
        .expect("service builds");
    for (path, expected) in [("/pets/rex", 200), ("/pets/rex/", 404)] {
        let response: http::Response<BoxBody> = app
            .clone()
            .oneshot(http::Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, expected, "{path}: {body:?}");
    }
}
