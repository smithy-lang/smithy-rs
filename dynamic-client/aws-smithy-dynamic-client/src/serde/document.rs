/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This module converts between documents and the schema serializers of a protocol.
//
// A document has no blob, timestamp, or big number value, so a member of those types fails with
// an `unsupported` error in both directions.
// TODO(dynamic-client): Use DocumentV2 from the `feature/documentv2` branch throughout the crate,
// for inputs, outputs, and this conversion. DocumentV2 holds blob and timestamp values.
// TODO(dynamic-client): Shape output documents by the output schema, including blobs,
// timestamps, and big numbers.

mod coerce;
mod correct;

pub(crate) use coerce::check_input;
use correct::fill_missing;

use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::shape::MemberView;
use aws_smithy_lang::ShapeId;
use aws_smithy_schema::serde::{
    SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::{Schema, ShapeType};
use aws_smithy_types::{Document, Number};
use std::collections::HashMap;

// `SerializableDocument` holds a structure or union document. A protocol's serializer writes
// the document through the document's schema. The document was checked against the schema by
// `check_input`, so only a value the check let through reaches the fallback errors below.
pub(crate) struct SerializableDocument<'a> {
    document: &'a Document,
    schema: &'a Schema<'a>,
    schemas: &'a RuntimeSchemaSet,
}

impl<'a> SerializableDocument<'a> {
    // `schema` is a structure or union schema, or a member schema that targets one.
    pub(crate) fn new(
        document: &'a Document,
        schema: &'a Schema<'a>,
        schemas: &'a RuntimeSchemaSet,
    ) -> Result<Self, SerdeError> {
        Ok(Self {
            document,
            schema: target_schema(schemas, schema)?,
            schemas,
        })
    }
}

impl SerializableStruct for SerializableDocument<'_> {
    fn schema(&self) -> &Schema<'_> {
        self.schema
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        let Document::Object(members) = self.document else {
            return Err(type_mismatch(self.schema, self.document));
        };
        for member in self.schema.members() {
            let name = member.member_name().expect("structure members have names");
            match members.get(name) {
                // A null member is unset, as in a generated input.
                None | Some(Document::Null) => {}
                Some(value) => write_value(self.schemas, member, value, serializer)?,
            }
        }
        Ok(())
    }
}

fn write_value(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    value: &Document,
    serializer: &mut dyn ShapeSerializer,
) -> Result<(), SerdeError> {
    match (schema.shape_type(), value) {
        (ShapeType::Boolean, Document::Bool(value)) => serializer.write_boolean(schema, *value),
        (ShapeType::Byte, Document::Number(number)) => {
            serializer.write_byte(schema, integer(schema, *number)?)
        }
        (ShapeType::Short, Document::Number(number)) => {
            serializer.write_short(schema, integer(schema, *number)?)
        }
        (ShapeType::Integer, Document::Number(number)) => {
            serializer.write_integer(schema, integer(schema, *number)?)
        }
        (ShapeType::Long, Document::Number(number)) => {
            serializer.write_long(schema, integer(schema, *number)?)
        }
        (ShapeType::Float, Document::Number(number)) => {
            serializer.write_float(schema, number.to_f32_lossy())
        }
        (ShapeType::Double, Document::Number(number)) => {
            serializer.write_double(schema, number.to_f64_lossy())
        }
        (ShapeType::String, Document::String(value)) => serializer.write_string(schema, value),
        (ShapeType::Document, value) => serializer.write_document(schema, value),
        (ShapeType::List, Document::Array(items)) => {
            let member = schema.member().expect("list schemas have a member");
            serializer.write_list(schema, &|serializer| {
                for item in items {
                    match item {
                        Document::Null => serializer.write_null(member)?,
                        item => write_value(schemas, member, item, serializer)?,
                    }
                }
                Ok(())
            })
        }
        (ShapeType::Map, Document::Object(entries)) => {
            let key = schema.key().expect("map schemas have a key");
            let member = schema.member().expect("map schemas have a value");
            serializer.write_map(schema, &|serializer| {
                for (name, entry) in entries {
                    serializer.write_string(key, name)?;
                    match entry {
                        Document::Null => serializer.write_null(member)?,
                        entry => write_value(schemas, member, entry, serializer)?,
                    }
                }
                Ok(())
            })
        }
        (ShapeType::Structure | ShapeType::Union, value) => {
            serializer.write_struct(schema, &SerializableDocument::new(value, schema, schemas)?)
        }
        (_, value) => Err(type_mismatch(schema, value)),
    }
}

// Reads a structure or union through a protocol's deserializer into a document object.
pub(crate) fn read_struct(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    deserializer: &mut dyn ShapeDeserializer,
) -> Result<Document, SerdeError> {
    let schema = target_schema(schemas, schema)?;
    let mut members = HashMap::new();
    deserializer.read_struct(schema, &mut |member, deserializer| {
        // A member the schema doesn't know, such as one added to the service later, is skipped.
        let Some(name) = member.member_name() else {
            return deserializer.skip_value();
        };
        if deserializer.is_null() {
            return deserializer.read_null();
        }
        members.insert(name.to_owned(), read_value(schemas, member, deserializer)?);
        Ok(())
    })?;
    fill_missing(schemas, schema, &mut members);
    Ok(Document::Object(members))
}

fn read_value(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    deserializer: &mut dyn ShapeDeserializer,
) -> Result<Document, SerdeError> {
    let value = match schema.shape_type() {
        ShapeType::Boolean => Document::Bool(deserializer.read_boolean(schema)?),
        ShapeType::Byte => signed(deserializer.read_byte(schema)?.into()),
        ShapeType::Short => signed(deserializer.read_short(schema)?.into()),
        ShapeType::Integer => signed(deserializer.read_integer(schema)?.into()),
        ShapeType::Long => signed(deserializer.read_long(schema)?),
        ShapeType::Float => {
            Document::Number(Number::Float(deserializer.read_float(schema)?.into()))
        }
        ShapeType::Double => Document::Number(Number::Float(deserializer.read_double(schema)?)),
        ShapeType::String => Document::String(deserializer.read_string(schema)?),
        ShapeType::Document => deserializer.read_document(schema)?,
        ShapeType::List => {
            let member = schema.member().expect("list schemas have a member");
            let mut items = Vec::new();
            deserializer.read_list(schema, &mut |deserializer| {
                items.push(read_item(schemas, member, deserializer)?);
                Ok(())
            })?;
            Document::Array(items)
        }
        ShapeType::Map => {
            let member = schema.member().expect("map schemas have a value");
            let mut entries = HashMap::new();
            deserializer.read_map(schema, &mut |name, deserializer| {
                entries.insert(name, read_item(schemas, member, deserializer)?);
                Ok(())
            })?;
            Document::Object(entries)
        }
        ShapeType::Structure | ShapeType::Union => read_struct(schemas, schema, deserializer)?,
        _ => return Err(unsupported(schema)),
    };
    Ok(value)
}

// Reads a list item or a map value. A null item stays in the document.
fn read_item(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    deserializer: &mut dyn ShapeDeserializer,
) -> Result<Document, SerdeError> {
    if deserializer.is_null() {
        deserializer.read_null()?;
        return Ok(Document::Null);
    }
    read_value(schemas, schema, deserializer)
}

// Returns the schema of the structure or union that a member schema targets, or the schema
// itself when it is not a member. A member schema has no members of its target, so the target is
// looked up in the model.
// TODO(aws-smithy-lang): Read the target schema of a member from `RuntimeSchemaSet` directly.
// Then the model lookup goes away, along with the parse of the member ID into a model `ShapeId`.
fn target_schema<'s>(
    schemas: &'s RuntimeSchemaSet,
    schema: &'s Schema<'s>,
) -> Result<&'s Schema<'s>, SerdeError> {
    if schema.member_name().is_none() {
        return Ok(schema);
    }
    model_member(schemas, schema)
        .and_then(|member| schemas.schema(member.target()))
        .ok_or_else(|| {
            SerdeError::custom(format!(
                "no schema for the target of member `{}`",
                schema.shape_id().as_str()
            ))
        })
}

// Returns the model member of a member schema.
// TODO(aws-smithy-lang): Drop this lookup once runtime schemas carry every model trait, such as
// `@required` and `@default`.
fn model_member<'s>(schemas: &'s RuntimeSchemaSet, schema: &Schema<'_>) -> Option<MemberView<'s>> {
    let id = ShapeId::new(schema.shape_id().as_str()).ok()?;
    schemas.model().get_shape(&id)?.as_member()
}

fn integer<T: TryFrom<Number>>(schema: &Schema<'_>, number: Number) -> Result<T, SerdeError> {
    T::try_from(number).map_err(|_| {
        SerdeError::invalid_input(format!(
            "{number:?} does not fit `{}`",
            schema.shape_id().as_str()
        ))
    })
}

fn signed(value: i64) -> Document {
    match u64::try_from(value) {
        Ok(value) => Document::Number(Number::PosInt(value)),
        Err(_) => Document::Number(Number::NegInt(value)),
    }
}

fn unsupported(schema: &Schema<'_>) -> SerdeError {
    SerdeError::unsupported(format!(
        "a document can't hold a value of `{}`",
        schema.shape_id().as_str()
    ))
}

fn type_mismatch(schema: &Schema<'_>, value: &Document) -> SerdeError {
    let kind = match value {
        Document::Object(_) => "an object",
        Document::Array(_) => "an array",
        Document::Number(_) => "a number",
        Document::String(_) => "a string",
        Document::Bool(_) => "a boolean",
        Document::Null => "null",
    };
    SerdeError::type_mismatch(format!(
        "`{}` can't take {kind}",
        schema.shape_id().as_str()
    ))
}
