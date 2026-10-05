/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Server RPC v2 CBOR serialization policy, layered over the ordinary CBOR codec.
//!
//! A structure-prefix hook emits error discriminators before modeled members,
//! including errors nested through structures, lists, maps, and unions.

use aws_smithy_cbor::codec::{CborCodec, CborDeserializer, CborSerializer};
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, ShapeSerializer};
use aws_smithy_schema::Schema;

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
    type Serializer = CborSerializer;
    type Deserializer<'a> = CborDeserializer<'a>;

    fn create_serializer(&self) -> Self::Serializer {
        self.0.create_serializer().with_struct_prefix(write_error_type)
    }

    fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a> {
        self.0.create_deserializer(input)
    }
}

fn write_error_type(schema: &Schema<'_>, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
    if schema
        .traits()
        .is_some_and(|traits| traits.contains_fqn("smithy.api#error"))
    {
        serializer.write_string(&TYPE_MEMBER, schema.shape_id().as_str())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::codec::FinishSerializer;
    use aws_smithy_schema::serde::SerializableStruct;
    use aws_smithy_schema::{shape_id, ShapeType};

    #[test]
    fn ordinary_structure_has_no_discriminator() {
        static NAME: Schema = Schema::new_member(shape_id!("test", "Output", "name"), ShapeType::String, "name", 0);
        static OUTPUT: Schema = Schema::new_struct(shape_id!("test", "Output"), ShapeType::Structure, &[&NAME]);
        struct Output;
        impl SerializableStruct for Output {
            fn schema(&self) -> &Schema<'_> {
                &OUTPUT
            }

            fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                serializer.write_string(&NAME, "hello")
            }
        }
        let mut serializer = RpcV2CborSerde::default().create_serializer();
        serializer.write_struct(&OUTPUT, &Output).unwrap();
        let mut expected = aws_smithy_cbor::Encoder::new(Vec::new());
        expected.begin_map().str("name").str("hello").end();
        assert_eq!(serializer.finish(), expected.into_writer());
    }

    #[test]
    fn only_the_server_adds_error_types() {
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
            Schema::new_member(shape_id!("test", "Output", "error"), ShapeType::Structure, "error", 0);
        static OUTPUT: Schema<'static> =
            Schema::new_struct(shape_id!("test", "Output"), ShapeType::Structure, &[&MEMBER]);
        struct Error;
        impl SerializableStruct for Error {
            fn schema(&self) -> &Schema<'_> {
                &ERROR
            }

            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_string(&MESSAGE, "failed")
            }
        }
        struct Output;
        impl SerializableStruct for Output {
            fn schema(&self) -> &Schema<'_> {
                &OUTPUT
            }
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_struct(&MEMBER, &Error)
            }
        }
        for enabled in [false, true] {
            let plain = CborCodec::default();
            let server = RpcV2CborSerde::default();
            let codec: &dyn aws_smithy_schema::codec::DynCodec = if enabled { &server } else { &plain };
            for nesting in 0..6 {
                let mut serializer = codec.create_serializer();
                let list = Schema::new(shape_id!("test", "Errors"), ShapeType::List);
                let map = Schema::new(shape_id!("test", "ErrorsByName"), ShapeType::Map);
                let union_members = [&MEMBER];
                let union = Schema::new_struct(shape_id!("test", "ErrorUnion"), ShapeType::Union, &union_members);
                let write_error = |ser: &mut dyn ShapeSerializer| ser.write_struct(&MEMBER, &Error);
                let write_entry = |ser: &mut dyn ShapeSerializer| {
                    ser.write_string(&aws_smithy_schema::prelude::STRING, "error")?;
                    write_error(ser)
                };
                match nesting {
                    0 => serializer.write_struct(&ERROR, &Error),
                    1 => serializer.write_struct(&OUTPUT, &Output),
                    2 => serializer.write_list(&list, &write_error),
                    3 => serializer.write_map(&map, &write_entry),
                    4 => serializer.write_struct(&union, &Output),
                    _ => serializer.write_map(&map, &|ser| {
                        ser.write_string(&aws_smithy_schema::prelude::STRING, "error")?;
                        ser.write_list(&list, &write_error)
                    }),
                }
                .unwrap();
                let actual = serializer.finish_boxed();
                // Exact bytes also prove ordering and absence of duplicate discriminator keys.
                let mut expected = aws_smithy_cbor::Encoder::new(Vec::new());
                match nesting {
                    1 | 3 | 4 => {
                        expected.begin_map().str("error");
                    }
                    2 => {
                        expected.begin_array();
                    }
                    5 => {
                        expected.begin_map().str("error").begin_array();
                    }
                    _ => {}
                }
                expected.begin_map();
                if enabled {
                    expected.str("__type").str("test#Failure");
                }
                expected.str("message").str("failed").end();
                if nesting != 0 {
                    expected.end();
                }
                if nesting == 5 {
                    expected.end();
                }
                assert_eq!(actual, expected.into_writer(), "server={enabled}, nesting={nesting}");
            }
        }
    }
}
