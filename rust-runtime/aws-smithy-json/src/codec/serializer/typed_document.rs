/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed documents for [`JsonSerializer`]: a structure written with its discriminator.
//!
//! A typed document is the object that [`write_struct`](ShapeSerializer::write_struct) writes,
//! with a `__type` member first, holding the structure's absolute shape ID. Members are written
//! by their schemas, so `@jsonName`, `@timestampFormat`, and the codec settings apply as they do
//! to the structure itself. [`JsonDeserializer::read_discriminated_document`] and
//! [`read_discriminated_document_v2`] lift the `__type` member back into the discriminator.
//!
//! [`JsonDeserializer::read_discriminated_document`]: crate::codec::JsonDeserializer::read_discriminated_document
//! [`read_discriminated_document_v2`]: crate::codec::JsonDeserializer::read_discriminated_document_v2

use super::JsonSerializer;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct};
use aws_smithy_schema::Schema;

#[cfg(doc)]
use aws_smithy_schema::serde::ShapeSerializer;

/// The encoded object key of the discriminator, in the form that
/// [`JsonCodecSettings::member_key`](crate::codec::JsonCodecSettings::member_key) returns.
const TYPE_KEY: &str = "\"__type\":";

impl JsonSerializer {
    /// [`ShapeSerializer::write_typed_document`] for JSON. Checks the discriminator and the
    /// member keys before writing anything.
    pub(super) fn write_typed_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        let shape = value.schema();
        let id = shape.shape_id().as_str();
        if !crate::codec::is_valid_discriminator(id) {
            return Err(SerdeError::invalid_input(format!(
                "a typed document's discriminator {id:?} must be an absolute shape ID \
                 (namespace#Name)"
            )));
        }
        if let Some(member) = shape
            .members()
            .iter()
            .find(|member| self.settings.member_key(member) == Some(TYPE_KEY))
        {
            return Err(SerdeError::invalid_input(format!(
                "{id} can't be written as a typed document: its member {:?} has the key \
                 `__type`, which JSON uses for the discriminator",
                member.member_name().unwrap_or_default()
            )));
        }
        self.prefix(schema);
        self.write_struct_body(Some(id), value)
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::{JsonCodec, JsonCodecSettings};
    use aws_smithy_schema::codec::Codec;
    use aws_smithy_schema::prelude::{DOCUMENT, STRING};
    use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
    use aws_smithy_schema::traits::TimestampFormat;
    use aws_smithy_schema::{shape_id, Schema, ShapeId, ShapeType};
    use aws_smithy_types::DateTime;

    static NICKNAME: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Pet", "nickname"),
        ShapeType::String,
        "nickname",
        0,
    )
    .with_json_name("Nickname");
    static AT: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Pet", "at"),
        ShapeType::Timestamp,
        "at",
        1,
    )
    .with_timestamp_format(TimestampFormat::DateTime);
    static OWNER: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Pet", "owner"),
        ShapeType::Structure,
        "owner",
        2,
    );
    static PET: Schema<'static> = Schema::new_struct(
        shape_id!("smithy.example", "Pet"),
        ShapeType::Structure,
        &[&NICKNAME, &AT, &OWNER],
    );
    static OWNER_NAME: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Owner", "name"),
        ShapeType::String,
        "name",
        0,
    )
    .with_json_name("Name");
    static OWNER_SCHEMA: Schema<'static> = Schema::new_struct(
        shape_id!("smithy.example", "Owner"),
        ShapeType::Structure,
        &[&OWNER_NAME],
    );
    static EMPTY: Schema<'static> = Schema::new_struct(
        shape_id!("smithy.example", "Empty"),
        ShapeType::Structure,
        &[],
    );

    struct Pet;
    impl SerializableStruct for Pet {
        fn schema(&self) -> &Schema<'_> {
            &PET
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            ser.write_string(&NICKNAME, "Rex")?;
            ser.write_timestamp(&AT, &DateTime::from_secs(5))?;
            ser.write_struct(&OWNER, &Owner)
        }
    }

    struct Owner;
    impl SerializableStruct for Owner {
        fn schema(&self) -> &Schema<'_> {
            &OWNER_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            ser.write_string(&OWNER_NAME, "Sam")
        }
    }

    /// A structure with no members and any schema, such as one with an invalid shape ID.
    struct Members<'a>(&'a Schema<'a>);
    impl SerializableStruct for Members<'_> {
        fn schema(&self) -> &Schema<'_> {
            self.0
        }

        fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    fn codec(use_json_name: bool) -> JsonCodec {
        JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(use_json_name)
                .build(),
        )
    }

    fn write(
        use_json_name: bool,
        write: impl FnOnce(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<String, (SerdeError, String)> {
        let mut ser = codec(use_json_name).create_serializer();
        let result = write(&mut ser);
        let output = String::from_utf8(ser.finish()).unwrap();
        result.map(|()| output.clone()).map_err(|err| (err, output))
    }

    #[test]
    fn writes_the_discriminator_then_the_members_by_their_schemas() {
        for (use_json_name, members) in [
            (
                true,
                r#""Nickname":"Rex","at":"1970-01-01T00:00:05Z","owner":{"Name":"Sam"}}"#,
            ),
            (
                false,
                r#""nickname":"Rex","at":"1970-01-01T00:00:05Z","owner":{"name":"Sam"}}"#,
            ),
        ] {
            let typed = write(use_json_name, |ser| {
                ser.write_typed_document(&DOCUMENT, &Pet)
            });
            assert_eq!(
                typed.unwrap(),
                format!(r#"{{"__type":"smithy.example#Pet",{members}"#)
            );
            let plain = write(use_json_name, |ser| ser.write_struct(&DOCUMENT, &Pet));
            assert_eq!(plain.unwrap(), format!("{{{members}"));
        }
        assert_eq!(
            write(true, |ser| ser
                .write_typed_document(&DOCUMENT, &Members(&EMPTY)))
            .unwrap(),
            r#"{"__type":"smithy.example#Empty"}"#
        );
    }

    #[test]
    fn writes_in_member_positions_like_write_struct() {
        static DOC: Schema<'static> = Schema::new_member(
            shape_id!("smithy.example", "Holder", "doc"),
            ShapeType::Document,
            "doc",
            0,
        )
        .with_json_name("Doc");
        static LIST: Schema<'static> =
            Schema::new_list(shape_id!("smithy.example", "Docs"), &DOCUMENT);
        static MAP: Schema<'static> =
            Schema::new_map(shape_id!("smithy.example", "DocMap"), &STRING, &DOCUMENT);
        struct Holder;
        impl SerializableStruct for Holder {
            fn schema(&self) -> &Schema<'_> {
                &EMPTY
            }

            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_typed_document(&DOC, &Members(&EMPTY))?;
                ser.write_string(&NICKNAME, "after")
            }
        }
        let empty = r#"{"__type":"smithy.example#Empty"}"#;

        assert_eq!(
            write(true, |ser| ser.write_struct(&DOCUMENT, &Holder)).unwrap(),
            format!(r#"{{"Doc":{empty},"Nickname":"after"}}"#)
        );
        let list = write(true, |ser| {
            ser.write_list(&LIST, &|ser| {
                ser.write_typed_document(&DOCUMENT, &Members(&EMPTY))?;
                ser.write_typed_document(&DOCUMENT, &Members(&EMPTY))
            })
        });
        assert_eq!(list.unwrap(), format!("[{empty},{empty}]"));
        let map = write(true, |ser| {
            ser.write_map(&MAP, &|ser| {
                ser.write_string(&STRING, "a")?;
                ser.write_typed_document(&DOCUMENT, &Members(&EMPTY))?;
                ser.write_string(&STRING, "b")?;
                ser.write_typed_document(&DOCUMENT, &Members(&EMPTY))
            })
        });
        assert_eq!(map.unwrap(), format!(r#"{{"a":{empty},"b":{empty}}}"#));
    }

    #[test]
    fn rejects_a_shape_id_that_is_not_absolute() {
        let relative = Schema::new_struct(
            ShapeId::from_parts("Empty", "", "Empty"),
            ShapeType::Structure,
            &[],
        );
        for schema in [&relative, &NICKNAME] {
            let (err, output) = write(true, |ser| {
                ser.write_typed_document(&DOCUMENT, &Members(schema))
            })
            .unwrap_err();
            assert!(
                matches!(&err, SerdeError::InvalidInput { message, .. } if message.contains("absolute")),
                "{err:?}"
            );
            assert_eq!(output, "");
        }
    }

    #[test]
    fn rejects_a_member_with_the_discriminator_key() {
        let named = Schema::new_member(
            shape_id!("smithy.example", "Typed", "__type"),
            ShapeType::String,
            "__type",
            0,
        );
        let renamed = Schema::new_member(
            shape_id!("smithy.example", "Typed", "kind"),
            ShapeType::String,
            "kind",
            0,
        )
        .with_json_name("__type");
        let named_members = [&named];
        let renamed_members = [&renamed];
        let named_shape = Schema::new_struct(
            shape_id!("smithy.example", "Typed"),
            ShapeType::Structure,
            &named_members,
        );
        let renamed_shape = Schema::new_struct(
            shape_id!("smithy.example", "Typed"),
            ShapeType::Structure,
            &renamed_members,
        );

        for (shape, use_json_name, rejected) in [
            (&named_shape, true, true),
            (&named_shape, false, true),
            (&renamed_shape, true, true),
            // Without `@jsonName`, the member's key is `kind`.
            (&renamed_shape, false, false),
        ] {
            let result = write(use_json_name, |ser| {
                ser.write_typed_document(&DOCUMENT, &Members(shape))
            });
            match result {
                Err((SerdeError::InvalidInput { message, .. }, output)) if rejected => {
                    assert!(message.contains("`__type`"), "{message}");
                    assert_eq!(output, "");
                }
                Ok(output) if !rejected => {
                    assert_eq!(output, r#"{"__type":"smithy.example#Typed"}"#)
                }
                other => panic!("use_json_name: {use_json_name}: {other:?}"),
            }
        }
    }
}
