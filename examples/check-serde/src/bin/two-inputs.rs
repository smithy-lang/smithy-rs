// namespace example
//
// @http(method: "GET", uri: "/demo")
// operation Demo {
//     input := {
//         @httpHeader("x-start")
//         start: String
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
//         region: String
//     }
// }
//
use aws_smithy_json::codec::JsonCodec;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::codec::http_string::HttpStringDeserializer;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeId, ShapeType};
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use http::Request;

static START_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "start",
    0,
)
.with_http_header("x-start");

static MODE_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
    ShapeId::from_parts("example#String", "example", "String"),
    ShapeType::String,
    "mode",
    1,
);

static INPUT_MEMBERS: [&Schema<'static>; 2] = [&START_MEMBER_SCHEMA, &MODE_MEMBER_SCHEMA];

static INPUT_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("example#Input", "example", "Input"),
    ShapeType::Structure,
    &INPUT_MEMBERS,
);

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
    1,
);

static OTHER_INPUT_MEMBERS: [&Schema<'static>; 2] = [&SOURCE_MEMBER_SCHEMA, &REGION_MEMBER_SCHEMA];

static OTHER_INPUT_SCHEMA: Schema<'static> = Schema::new_struct(
    ShapeId::from_parts("example#OtherInput", "example", "OtherInput"),
    ShapeType::Structure,
    &OTHER_INPUT_MEMBERS,
);

#[derive(Debug, Default)]
struct Input {
    start: Option<String>,
    mode: Option<String>,
}

impl Input {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
        let mut out = Self::default();
        deserializer.read_struct(&INPUT_SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    out.start = Some(deser.read_string(member)?);
                }
                Some(1) => {
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
    region: Option<String>,
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
                    out.region = Some(deser.read_string(member)?);
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(out)
    }
}

struct RequestDeserializer<'a> {
    request: &'a Request<Vec<u8>>,
    codec: JsonCodec,
}

impl<'a> RequestDeserializer<'a> {
    fn new(request: &'a Request<Vec<u8>>) -> Self {
        Self {
            request,
            codec: JsonCodec::default(),
        }
    }
}

fn is_body_member(member: &Schema<'_>) -> bool {
    member.http_header().is_none()
}

impl ShapeDeserializer for RequestDeserializer<'_> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let mut has_body_members = false;

        for member in schema.members() {
            if let Some(header) = member.http_header() {
                let header_name = header.value();
                if let Some(value) = self.request.headers().get(header_name) {
                    let value = value.to_str().map_err(|err| {
                        SerdeError::invalid_input(format!(
                            "invalid header value for `{header_name}`: {err}"
                        ))
                    })?;
                    let mut value_deserializer = HttpStringDeserializer::new(value);
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
fn main() -> Result<(), SerdeError> {
    let demo_request = Request::builder()
        .method("GET")
        .uri("/demo")
        .header("x-start", "from-header")
        .header("content-type", "application/json")
        .body(br#"{"mode":"dry-run"}"#.to_vec())
        .map_err(|err| SerdeError::invalid_input(format!("invalid request: {err}")))?;
    let mut deserializer = RequestDeserializer::new(&demo_request);

    let input = Input::deserialize(&mut deserializer)?;
    println!("{input:?}");

    let other_request = Request::builder()
        .method("POST")
        .uri("/other")
        .header("x-source", "integration-test")
        .header("content-type", "application/json")
        .body(br#"{"region":"us-east-1"}"#.to_vec())
        .map_err(|err| SerdeError::invalid_input(format!("invalid request: {err}")))?;
    let mut deserializer = RequestDeserializer::new(&other_request);

    let other_input = OtherInput::deserialize(&mut deserializer)?;
    println!("{other_input:?}");

    Ok(())
}
