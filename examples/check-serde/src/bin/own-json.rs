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
    body: serde_json::Value,
}

impl<'a> RequestDeserializer<'a> {
    fn new(request: &'a Request<Vec<u8>>) -> Result<Self, SerdeError> {
        let body = serde_json::from_slice(request.body())
            .map_err(|err| SerdeError::invalid_input(format!("invalid JSON body: {err}")))?;
        Ok(Self { request, body })
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

            if !is_body_member(member) {
                continue;
            }

            let Some(member_name) = member.member_name() else {
                continue;
            };
            let Some(value) = self.body.get(member_name) else {
                continue;
            };
            let mut value_deserializer = JsonValueDeserializer::new(value);
            consumer(member, &mut value_deserializer)?;
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

struct JsonValueDeserializer<'a> {
    value: &'a serde_json::Value,
}

impl<'a> JsonValueDeserializer<'a> {
    fn new(value: &'a serde_json::Value) -> Self {
        Self { value }
    }
}

impl ShapeDeserializer for JsonValueDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(
            &Schema<'_>,
            &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "no nested struct support in this demo",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("no list support in this demo"))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("no map support in this demo"))
    }

    fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
        self.value
            .as_bool()
            .ok_or_else(|| SerdeError::invalid_input("expected JSON boolean"))
    }

    fn read_byte(&mut self, schema: &Schema<'_>) -> Result<i8, SerdeError> {
        self.read_long(schema).and_then(|value| {
            i8::try_from(value).map_err(|_| SerdeError::invalid_input("expected JSON byte"))
        })
    }

    fn read_short(&mut self, schema: &Schema<'_>) -> Result<i16, SerdeError> {
        self.read_long(schema).and_then(|value| {
            i16::try_from(value).map_err(|_| SerdeError::invalid_input("expected JSON short"))
        })
    }

    fn read_integer(&mut self, schema: &Schema<'_>) -> Result<i32, SerdeError> {
        self.read_long(schema).and_then(|value| {
            i32::try_from(value).map_err(|_| SerdeError::invalid_input("expected JSON integer"))
        })
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        self.value
            .as_i64()
            .ok_or_else(|| SerdeError::invalid_input("expected JSON long"))
    }

    fn read_float(&mut self, schema: &Schema<'_>) -> Result<f32, SerdeError> {
        self.read_double(schema).map(|value| value as f32)
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        self.value
            .as_f64()
            .ok_or_else(|| SerdeError::invalid_input("expected JSON double"))
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        Err(SerdeError::unsupported(
            "no big integer support in this demo",
        ))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        Err(SerdeError::unsupported(
            "no big decimal support in this demo",
        ))
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        self.value
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| SerdeError::invalid_input("expected JSON string"))
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        Err(SerdeError::unsupported("no blob support in this demo"))
    }

    fn read_timestamp(&mut self, _schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        Err(SerdeError::unsupported("no timestamp support in this demo"))
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported("no document support in this demo"))
    }

    fn is_null(&self) -> bool {
        self.value.is_null()
    }

    fn container_size(&self) -> Option<usize> {
        match self.value {
            serde_json::Value::Array(values) => Some(values.len()),
            serde_json::Value::Object(values) => Some(values.len()),
            _ => None,
        }
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
    let mut deserializer = RequestDeserializer::new(&demo_request)?;

    let input = Input::deserialize(&mut deserializer)?;
    println!("{input:?}");

    let other_request = Request::builder()
        .method("POST")
        .uri("/other")
        .header("x-source", "integration-test")
        .header("content-type", "application/json")
        .body(br#"{"region":"us-east-1"}"#.to_vec())
        .map_err(|err| SerdeError::invalid_input(format!("invalid request: {err}")))?;
    let mut deserializer = RequestDeserializer::new(&other_request)?;

    let other_input = OtherInput::deserialize(&mut deserializer)?;
    println!("{other_input:?}");

    Ok(())
}
