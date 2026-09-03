// namespace example
//
// @http(method: "GET", uri: "/demo/{id}")
// operation Demo {
//     input := {
//         @httpLabel
//         id: String
//
//         @httpHeader("x-start")
//         start: String
//
//         @httpHeader("x-trace-id")
//         traceId: String
//
//         mode: String
//     }
// }
//
// @http(method: "POST", uri: "/other")
// operation OtherDemo {
//     input := {
//         @httpHeader("x-source")
//         source: String
//
//         @httpHeader("x-tenant")
//         tenant: StringList
//
//         @httpQuery("kind")
//         kind: String
//
//         @httpPrefixHeaders("x-meta-")
//         metadata: StringMap
//
//         region: String
//     }
// }
//
mod deserializers {
    pub(crate) mod header_values;
}

use aws_smithy_json::codec::JsonCodec;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::codec::http_string::HttpStringDeserializer;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{Schema, ShapeId, ShapeType};
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use deserializers::header_values::{
    HeaderValuesDeserializer, PrefixHeadersDeserializer, ScalarHeaderValueDeserializer,
};
use http::Request;
use std::collections::HashMap;

static START_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "start",
    1,
)
.with_http_header("x-start");

static MODE_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "mode",
    3,
);

static TRACE_ID_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "traceId",
    2,
)
.with_http_header("x-trace-id");

static ID_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "id",
    0,
)
.with_http_label();

static INPUT_MEMBERS: [&Schema<'static>; 4] = [
    &ID_MEMBER_SCHEMA,
    &START_MEMBER_SCHEMA,
    &TRACE_ID_MEMBER_SCHEMA,
    &MODE_MEMBER_SCHEMA,
];

static INPUT_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("example#Input", "example", "Input"),
    ShapeType::Structure,
    &INPUT_MEMBERS,
)
.with_http(HttpTrait::new("GET", "/demo/{id}", Some(200)));

static SOURCE_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "source",
    0,
)
.with_http_header("x-source");

static REGION_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "region",
    3,
);

static TENANT_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#StringList", "example", "StringList"),
    ShapeType::List,
    "tenant",
    1,
)
.with_list_member(&STRING_LIST_MEMBER_SCHEMA)
.with_http_header("x-tenant");

static STRING_LIST_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "member",
    0,
);

static KIND_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "kind",
    2,
)
.with_http_query("kind");

static STRING_MAP_KEY_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "key",
    0,
);

static STRING_MAP_VALUE_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "value",
    1,
);

static METADATA_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#StringMap", "example", "StringMap"),
    ShapeType::Map,
    "metadata",
    4,
)
.with_map_members(
    &STRING_MAP_KEY_MEMBER_SCHEMA,
    &STRING_MAP_VALUE_MEMBER_SCHEMA,
)
.with_http_prefix_headers("x-meta-");

static OTHER_INPUT_MEMBERS: [&Schema<'static>; 5] = [
    &SOURCE_MEMBER_SCHEMA,
    &TENANT_MEMBER_SCHEMA,
    &KIND_MEMBER_SCHEMA,
    &REGION_MEMBER_SCHEMA,
    &METADATA_MEMBER_SCHEMA,
];

static OTHER_INPUT_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("example#OtherInput", "example", "OtherInput"),
    ShapeType::Structure,
    &OTHER_INPUT_MEMBERS,
);

#[derive(Debug, Default)]
struct Input {
    id: Option<String>,
    start: Option<String>,
    trace_id: Option<String>,
    mode: Option<String>,
}

impl Input {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
        let mut out = Self::default();
        deserializer.read_struct(&INPUT_SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    out.id = Some(deser.read_string(member)?);
                }
                Some(1) => {
                    out.start = Some(deser.read_string(member)?);
                }
                Some(2) => {
                    out.trace_id = Some(deser.read_string(member)?);
                }
                Some(3) => {
                    out.mode = Some(deser.read_string(member)?);
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(out)
    }
}

#[derive(Debug, Default)]
struct OtherInput {
    source: Option<String>,
    tenant: Vec<String>,
    kind: Option<String>,
    region: Option<String>,
    metadata: HashMap<String, String>,
}

impl OtherInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
        let mut out = Self::default();
        deserializer.read_struct(&OTHER_INPUT_SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    out.source = Some(deser.read_string(member)?);
                }
                Some(1) => {
                    deser.read_list(member, &mut |element_deser| {
                        out.tenant.push(element_deser.read_string(member)?);
                        Ok(())
                    })?;
                }
                Some(2) => {
                    out.kind = Some(deser.read_string(member)?);
                }
                Some(3) => {
                    out.region = Some(deser.read_string(member)?);
                }
                Some(4) => {
                    deser.read_map(member, &mut |key, value_deser| {
                        out.metadata.insert(
                            key,
                            value_deser.read_string(
                                member.member().unwrap_or(&STRING_MAP_VALUE_MEMBER_SCHEMA),
                            )?,
                        );
                        Ok(())
                    })?;
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(out)
    }
}

struct RequestDeserializer<'a, C = JsonCodec> {
    request: &'a Request<Vec<u8>>,
    codec: C,
}

impl<'a> RequestDeserializer<'a, JsonCodec> {
    fn new(request: &'a Request<Vec<u8>>) -> Self {
        Self::new_with_codec(request, JsonCodec::default())
    }
}

impl<'a, C> RequestDeserializer<'a, C>
where
    C: Codec,
{
    fn new_with_codec(request: &'a Request<Vec<u8>>, codec: C) -> Self {
        Self { request, codec }
    }
}

fn is_body_member(member: &Schema<'_>) -> bool {
    member.http_header().is_none()
        && member.http_label().is_none()
        && member.http_query().is_none()
        && member.http_prefix_headers().is_none()
}

fn label_value<'a>(
    schema: &Schema<'_>,
    request: &'a Request<Vec<u8>>,
    label_name: &str,
) -> Option<&'a str> {
    let template = schema.http()?.uri();
    let template_segments: Vec<&str> = template.trim_matches('/').split('/').collect();
    let path_segments: Vec<&str> = request.uri().path().trim_matches('/').split('/').collect();

    for (template_segment, path_segment) in template_segments.into_iter().zip(path_segments) {
        if template_segment == format!("{{{label_name}}}") {
            return Some(path_segment);
        }
    }

    None
}

fn query_value<'a>(request: &'a Request<Vec<u8>>, query_name: &str) -> Option<&'a str> {
    request.uri().query()?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name == query_name).then_some(value)
    })
}

impl<C> ShapeDeserializer for RequestDeserializer<'_, C>
where
    C: Codec,
{
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let mut has_body_members = false;

        for member in schema.members() {
            if member.http_label().is_some() {
                let member_name = member.member_name().unwrap_or_default();
                if let Some(value) = label_value(schema, self.request, member_name) {
                    let mut value_deserializer = HttpStringDeserializer::new(value);
                    consumer(member, &mut value_deserializer)?;
                }
                continue;
            }

            if let Some(header) = member.http_header() {
                let header_name = header.value();
                let member_is_list = member.shape_type() == ShapeType::List;
                let mut header_values = self
                    .request
                    .headers()
                    .get_all(header_name)
                    .iter()
                    .map(|value| value.to_str());
                let first = match header_values.next() {
                    Some(value) => value.map_err(|err| {
                        SerdeError::invalid_input(format!(
                            "invalid header value for `{header_name}`: {err}"
                        ))
                    })?,
                    None => continue,
                };
                if !member_is_list {
                    if let Some(value) = header_values.next() {
                        value.map_err(|err| {
                            SerdeError::invalid_input(format!(
                                "invalid header value for `{header_name}`: {err}"
                            ))
                        })?;
                        return Err(SerdeError::invalid_input(
                            "expected a single header value but found multiple",
                        ));
                    }
                    let mut value_deserializer = ScalarHeaderValueDeserializer::new(member, first);
                    consumer(member, &mut value_deserializer)?;
                    continue;
                }
                let second = match header_values.next() {
                    Some(value) => value.map_err(|err| {
                        SerdeError::invalid_input(format!(
                            "invalid header value for `{header_name}`: {err}"
                        ))
                    })?,
                    None => {
                        let mut value_deserializer = HeaderValuesDeserializer::one(member, first);
                        consumer(member, &mut value_deserializer)?;
                        continue;
                    }
                };
                let mut values = vec![first, second];
                for value in header_values {
                    values.push(value.map_err(|err| {
                        SerdeError::invalid_input(format!(
                            "invalid header value for `{header_name}`: {err}"
                        ))
                    })?);
                }
                let mut value_deserializer = HeaderValuesDeserializer::new(member, values);
                consumer(member, &mut value_deserializer)?;
                continue;
            }

            if let Some(query) = member.http_query() {
                if let Some(value) = query_value(self.request, query.value()) {
                    let mut value_deserializer = HttpStringDeserializer::new(value);
                    consumer(member, &mut value_deserializer)?;
                }
                continue;
            }

            if let Some(prefix) = member.http_prefix_headers() {
                let prefix = prefix.value();
                let mut value_deserializer =
                    PrefixHeadersDeserializer::from_headers(member, prefix, self.request.headers());
                if !value_deserializer.is_empty() {
                    consumer(member, &mut value_deserializer)?;
                }
                continue;
            }

            if is_body_member(member) {
                has_body_members = true;
            }
        }

        if has_body_members && !self.request.body().is_empty() {
            let mut body_deserializer = self.codec.create_deserializer(self.request.body());
            body_deserializer.read_struct(schema, consumer)?;
        }

        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_timestamp(&mut self, _schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported(
            "operation input must be a structure",
        ))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_cbor::Encoder;
    use aws_smithy_cbor::codec::CborCodec;

    fn input_from(request: &Request<Vec<u8>>) -> Result<Input, SerdeError> {
        let mut deserializer = RequestDeserializer::new(request);
        Input::deserialize(&mut deserializer)
    }

    fn other_input_from(request: &Request<Vec<u8>>) -> Result<OtherInput, SerdeError> {
        let mut deserializer = RequestDeserializer::new(request);
        OtherInput::deserialize(&mut deserializer)
    }

    fn other_input_from_with_codec<C: Codec>(
        request: &Request<Vec<u8>>,
        codec: C,
    ) -> Result<OtherInput, SerdeError> {
        let mut deserializer = RequestDeserializer::new_with_codec(request, codec);
        OtherInput::deserialize(&mut deserializer)
    }

    fn cbor_region_body(region: &str) -> Vec<u8> {
        let mut encoder = Encoder::new(Vec::new());
        encoder.begin_map().str("region").str(region).end();
        encoder.into_writer()
    }

    #[test]
    fn input_reads_label_scalar_headers_and_json_body() {
        let request = Request::builder()
            .method("GET")
            .uri("/demo/demo-123")
            .header("x-start", "from-header")
            .header("x-trace-id", "trace-123")
            .header("content-type", "application/json")
            .body(br#"{"mode":"dry-run"}"#.to_vec())
            .unwrap();

        let input = input_from(&request).unwrap();

        assert_eq!(input.id.as_deref(), Some("demo-123"));
        assert_eq!(input.start.as_deref(), Some("from-header"));
        assert_eq!(input.trace_id.as_deref(), Some("trace-123"));
        assert_eq!(input.mode.as_deref(), Some("dry-run"));
    }

    #[test]
    fn other_input_reads_query_list_header_prefix_headers_and_json_body() {
        let request = Request::builder()
            .method("POST")
            .uri("/other?kind=full")
            .header("x-source", "integration-test")
            .header("x-tenant", "tenant-a, \"tenant,b\"")
            .header("x-tenant", "tenant-c")
            .header("x-meta-color", "red")
            .header("x-meta-size", "large")
            .header("content-type", "application/json")
            .body(br#"{"region":"us-east-1"}"#.to_vec())
            .unwrap();

        let input = other_input_from(&request).unwrap();

        assert_eq!(input.source.as_deref(), Some("integration-test"));
        assert_eq!(input.tenant, ["tenant-a", "tenant,b", "tenant-c"]);
        assert_eq!(input.kind.as_deref(), Some("full"));
        assert_eq!(input.region.as_deref(), Some("us-east-1"));
        assert_eq!(input.metadata.get("color").map(String::as_str), Some("red"));
        assert_eq!(
            input.metadata.get("size").map(String::as_str),
            Some("large")
        );
    }

    #[test]
    fn other_input_can_read_cbor_body_with_http_bindings() {
        let request = Request::builder()
            .method("POST")
            .uri("/other?kind=compact")
            .header("x-source", "integration-test")
            .header("x-tenant", "tenant-a")
            .header("x-meta-color", "red")
            .header("content-type", "application/cbor")
            .body(cbor_region_body("us-west-2"))
            .unwrap();

        let input = other_input_from_with_codec(&request, CborCodec::default()).unwrap();

        assert_eq!(input.source.as_deref(), Some("integration-test"));
        assert_eq!(input.tenant, ["tenant-a"]);
        assert_eq!(input.kind.as_deref(), Some("compact"));
        assert_eq!(input.region.as_deref(), Some("us-west-2"));
        assert_eq!(input.metadata.get("color").map(String::as_str), Some("red"));
    }

    #[test]
    fn prefix_header_matching_is_case_insensitive() {
        let request = Request::builder()
            .method("POST")
            .uri("/other")
            .header("X-Meta-Color", "red")
            .body(Vec::new())
            .unwrap();

        let input = other_input_from(&request).unwrap();

        assert_eq!(input.metadata.get("color").map(String::as_str), Some("red"));
    }

    #[test]
    fn scalar_header_rejects_multiple_header_lines() {
        let request = Request::builder()
            .method("POST")
            .uri("/other")
            .header("x-source", "first")
            .header("x-source", "second")
            .body(Vec::new())
            .unwrap();

        let err = other_input_from(&request).unwrap_err();

        assert!(
            format!("{err:?}").contains("expected a single header value but found multiple"),
            "{err:?}"
        );
    }

    #[test]
    fn missing_json_body_leaves_body_members_unset() {
        let request = Request::builder()
            .method("GET")
            .uri("/demo/demo-123")
            .body(Vec::new())
            .unwrap();

        let input = input_from(&request).unwrap();

        assert_eq!(input.id.as_deref(), Some("demo-123"));
        assert_eq!(input.mode.as_deref(), None);
    }
}

fn main() -> Result<(), SerdeError> {
    let demo_request = Request::builder()
        .method("GET")
        .uri("/demo/demo-123")
        .header("x-start", "from-header")
        .header("x-trace-id", "trace-123")
        .header("content-type", "application/json")
        .body(br#"{"mode":"dry-run"}"#.to_vec())
        .map_err(|err| SerdeError::invalid_input(format!("invalid request: {err}")))?;
    let mut deserializer = RequestDeserializer::new(&demo_request);

    let input = Input::deserialize(&mut deserializer)?;
    println!("{input:?}");

    let other_request = Request::builder()
        .method("POST")
        .uri("/other?kind=full")
        .header("x-source", "integration-test")
        .header("x-tenant", "tenant-a")
        .header("x-tenant", "tenant-b")
        .header("x-meta-color", "red")
        .header("x-meta-size", "large")
        .header("content-type", "application/json")
        .body(br#"{"region":"us-east-1"}"#.to_vec())
        .map_err(|err| SerdeError::invalid_input(format!("invalid request: {err}")))?;
    let mut deserializer = RequestDeserializer::new(&other_request);

    let other_input = OtherInput::deserialize(&mut deserializer)?;
    println!("{other_input:?}");

    Ok(())
}
