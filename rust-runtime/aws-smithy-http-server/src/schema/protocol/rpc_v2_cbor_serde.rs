/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Server RPC v2 CBOR serialization policy, layered over the ordinary CBOR codec.
//!
//! Every aggregate callback must receive the adapter again: handing a modeled value
//! directly to the codec would let its nested structures bypass the error policy.

use aws_smithy_cbor::codec::{CborCodec, CborDeserializer, CborSerializer};
use aws_smithy_schema::codec::{Codec, FinishSerializer};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

use super::discriminator::TYPE_MEMBER;

/// Supplies the server serializer to both HTTP responses and event-stream payloads.
/// Deserialization and byte encoding remain the underlying codec's responsibility.
#[derive(Debug)]
pub(crate) struct RpcV2CborSerde(CborCodec);

impl Default for RpcV2CborSerde {
    fn default() -> Self {
        Self(CborCodec::new(
            aws_smithy_cbor::codec::CborCodecSettings::default().enforce_strictness(true),
        ))
    }
}

impl Codec for RpcV2CborSerde {
    type Serializer = RpcV2CborSerializer<CborSerializer>;
    type Deserializer<'a> = CborDeserializer<'a>;

    fn create_serializer(&self) -> Self::Serializer {
        RpcV2CborSerializer(self.0.create_serializer())
    }

    fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a> {
        self.0.create_deserializer(input)
    }
}

/// The outer adapter owns the byte serializer; aggregate callbacks borrow it.
pub(crate) struct RpcV2CborSerializer<S>(S);

pub(crate) trait SerializerStorage {
    fn serializer(&mut self) -> &mut dyn ShapeSerializer;
}

impl SerializerStorage for CborSerializer {
    fn serializer(&mut self) -> &mut dyn ShapeSerializer {
        self
    }
}

impl SerializerStorage for &mut dyn ShapeSerializer {
    fn serializer(&mut self) -> &mut dyn ShapeSerializer {
        *self
    }
}

impl<S: FinishSerializer> FinishSerializer for RpcV2CborSerializer<S> {
    fn finish(self) -> Vec<u8> {
        self.0.finish()
    }
}

/// The codec calls this after opening the structure's map. Even ordinary structures
/// use this wrapper so errors below them still pass through the server adapter.
struct Members<'a> {
    type_id: Option<&'a str>,
    value: &'a dyn SerializableStruct,
}

impl SerializableStruct for Members<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        if let Some(type_id) = self.type_id {
            serializer.write_string(&TYPE_MEMBER, type_id)?;
        }
        self.value.serialize_members(&mut RpcV2CborSerializer(serializer))
    }
}

impl<S: SerializerStorage> ShapeSerializer for RpcV2CborSerializer<S> {
    fn write_struct(&mut self, schema: &Schema<'_>, value: &dyn SerializableStruct) -> Result<(), SerdeError> {
        let target = schema.target().unwrap_or(schema);
        let type_id = target
            .traits()
            .is_some_and(|traits| traits.contains_fqn("smithy.api#error"))
            .then(|| target.shape_id().as_str());
        self.0.serializer().write_struct(schema, &Members { type_id, value })
    }

    fn write_list(
        &mut self,
        schema: &Schema<'_>,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.0.serializer().write_list(schema, &|serializer| {
            write_elements(&mut RpcV2CborSerializer(serializer))
        })
    }

    fn write_map(
        &mut self,
        schema: &Schema<'_>,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.0.serializer().write_map(schema, &|serializer| {
            write_entries(&mut RpcV2CborSerializer(serializer))
        })
    }

    fn write_boolean(&mut self, schema: &Schema<'_>, value: bool) -> Result<(), SerdeError> {
        self.0.serializer().write_boolean(schema, value)
    }

    fn write_byte(&mut self, schema: &Schema<'_>, value: i8) -> Result<(), SerdeError> {
        self.0.serializer().write_byte(schema, value)
    }

    fn write_short(&mut self, schema: &Schema<'_>, value: i16) -> Result<(), SerdeError> {
        self.0.serializer().write_short(schema, value)
    }

    fn write_integer(&mut self, schema: &Schema<'_>, value: i32) -> Result<(), SerdeError> {
        self.0.serializer().write_integer(schema, value)
    }

    fn write_long(&mut self, schema: &Schema<'_>, value: i64) -> Result<(), SerdeError> {
        self.0.serializer().write_long(schema, value)
    }

    fn write_float(&mut self, schema: &Schema<'_>, value: f32) -> Result<(), SerdeError> {
        self.0.serializer().write_float(schema, value)
    }

    fn write_double(&mut self, schema: &Schema<'_>, value: f64) -> Result<(), SerdeError> {
        self.0.serializer().write_double(schema, value)
    }

    fn write_big_integer(&mut self, schema: &Schema<'_>, value: &BigInteger) -> Result<(), SerdeError> {
        self.0.serializer().write_big_integer(schema, value)
    }

    fn write_big_decimal(&mut self, schema: &Schema<'_>, value: &BigDecimal) -> Result<(), SerdeError> {
        self.0.serializer().write_big_decimal(schema, value)
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        self.0.serializer().write_string(schema, value)
    }

    fn write_blob(&mut self, schema: &Schema<'_>, value: Blob) -> Result<(), SerdeError> {
        self.0.serializer().write_blob(schema, value)
    }

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        self.0.serializer().write_timestamp(schema, value)
    }

    fn write_document(&mut self, schema: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        self.0.serializer().write_document(schema, value)
    }

    fn write_null(&mut self, schema: &Schema<'_>) -> Result<(), SerdeError> {
        self.0.serializer().write_null(schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::{shape_id, ShapeType};

    #[test]
    fn only_the_server_adapter_adds_error_types() {
        static TRAITS: std::sync::LazyLock<aws_smithy_schema::TraitMap> = std::sync::LazyLock::new(|| {
            let mut traits = aws_smithy_schema::TraitMap::new();
            traits.insert(Box::new(aws_smithy_schema::StringTrait::new(
                shape_id!("smithy.api", "error"),
                "client",
            )));
            traits
        });
        static MESSAGE: Schema<'static> =
            Schema::new_member(shape_id!("test", "Failure", "message"), ShapeType::String, "message", 0);
        static ERROR: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Failure"), ShapeType::Structure, &[&MESSAGE]).with_traits(&TRAITS);
        static MEMBER: Schema<'static> =
            Schema::new_member(shape_id!("test", "Output", "error"), ShapeType::Structure, "error", 0)
                .with_target(|| &ERROR);
        static OUTPUT: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Output"), ShapeType::Structure, &[&MEMBER]);
        struct Error;
        impl SerializableStruct for Error {
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_string(&MESSAGE, "failed")
            }
        }
        struct Output;
        impl SerializableStruct for Output {
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_struct(&MEMBER, &Error)
            }
        }
        for enabled in [false, true] {
            let plain = CborCodec::default();
            let server = RpcV2CborSerde::default();
            let codec: &dyn aws_smithy_schema::codec::DynCodec = if enabled { &server } else { &plain };
            for nested in [false, true] {
                let mut serializer = codec.create_serializer();
                if nested {
                    serializer.write_struct(&OUTPUT, &Output).unwrap();
                } else {
                    serializer.write_struct(&ERROR, &Error).unwrap();
                }
                let actual = serializer.finish_boxed();
                // Exact bytes also prove ordering and absence of duplicate discriminator keys.
                let mut expected = aws_smithy_cbor::Encoder::new(Vec::new());
                if nested {
                    expected.begin_map().str("error");
                }
                expected.begin_map();
                if enabled {
                    expected.str("__type").str("test#Failure");
                }
                expected.str("message").str("failed").end();
                if nested {
                    expected.end();
                }
                assert_eq!(actual, expected.into_writer());
            }
        }
    }
}
