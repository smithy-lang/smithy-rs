/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This module checks an input document against the input schema, before the protocol
// serializes the document.

use super::{model_member, target_schema};
use crate::error::{InputError, InputErrorKind};
use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::ShapeId;
use aws_smithy_schema::{Schema, ShapeType};
use aws_smithy_types::{Document, Number};

// Checks that `input`, the members of an operation input, matches the input schema `schema`.
pub(crate) fn check_input(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    input: &Document,
) -> Result<(), InputError> {
    let mut path = String::new();
    check_value(schemas, schema, input, &mut path)
}

fn check_value(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    value: &Document,
    path: &mut String,
) -> Result<(), InputError> {
    let error = |kind| Err(InputError::new(path.clone(), schema, kind));
    match (schema.shape_type(), value) {
        (ShapeType::Boolean, Document::Bool(_))
        | (ShapeType::String, Document::String(_))
        | (ShapeType::Float | ShapeType::Double, Document::Number(_))
        | (ShapeType::Document, _) => Ok(()),
        (ShapeType::Byte, Document::Number(number)) => fits::<i8>(*number, error),
        (ShapeType::Short, Document::Number(number)) => fits::<i16>(*number, error),
        (ShapeType::Integer, Document::Number(number)) => fits::<i32>(*number, error),
        (ShapeType::Long, Document::Number(number)) => fits::<i64>(*number, error),
        (ShapeType::List, Document::Array(items)) => {
            let member = schema.member().expect("list schemas have a member");
            let sparse = is_sparse(schemas, schema);
            for (index, item) in items.iter().enumerate() {
                with_segment(path, &index.to_string(), |path| {
                    check_entry(schemas, member, item, sparse, path)
                })?;
            }
            Ok(())
        }
        (ShapeType::Map, Document::Object(entries)) => {
            let member = schema.member().expect("map schemas have a value");
            let sparse = is_sparse(schemas, schema);
            for (key, entry) in entries {
                with_segment(path, key, |path| {
                    check_entry(schemas, member, entry, sparse, path)
                })?;
            }
            Ok(())
        }
        (ShapeType::Structure | ShapeType::Union, Document::Object(members)) => {
            let Ok(target) = target_schema(schemas, schema) else {
                return error(InputErrorKind::Unsupported);
            };
            check_members(schemas, target, members, path)
        }
        // TODO(dynamic-client): Accept blob, timestamp, and big number values once inputs are
        // DocumentV2. A blob takes a binary value only, and never a string. A timestamp takes
        // epoch seconds as a number, or a date-time string, as in Smithy model nodes.
        (
            ShapeType::Blob | ShapeType::Timestamp | ShapeType::BigInteger | ShapeType::BigDecimal,
            _,
        ) => error(InputErrorKind::Unsupported),
        _ => error(InputErrorKind::WrongType),
    }
}

// Checks a list item or a map value. Only a `@sparse` list or map takes a null entry, as in a
// generated client.
fn check_entry(
    schemas: &RuntimeSchemaSet,
    member: &Schema<'_>,
    entry: &Document,
    sparse: bool,
    path: &mut String,
) -> Result<(), InputError> {
    match entry {
        Document::Null if sparse => Ok(()),
        Document::Null => Err(InputError::new(
            path.clone(),
            member,
            InputErrorKind::WrongType,
        )),
        entry => check_value(schemas, member, entry, path),
    }
}

// Returns whether the list or map that the member schema `schema` targets has `@sparse`.
// TODO(aws-smithy-schema): Read `@sparse` from the schema, once runtime schemas carry it.
fn is_sparse(schemas: &RuntimeSchemaSet, schema: &Schema<'_>) -> bool {
    model_member(schemas, schema).is_some_and(|member| member.target_shape().has_trait(&SPARSE))
}

const SPARSE: ShapeId = ShapeId::from_static("smithy.api#sparse");

fn check_members(
    schemas: &RuntimeSchemaSet,
    schema: &Schema<'_>,
    members: &std::collections::HashMap<String, Document>,
    path: &mut String,
) -> Result<(), InputError> {
    let set = members
        .iter()
        .filter(|(_, value)| !matches!(value, Document::Null));
    if schema.shape_type() == ShapeType::Union && set.clone().count() != 1 {
        return Err(InputError::new(
            path.clone(),
            schema,
            InputErrorKind::InvalidUnion,
        ));
    }
    for (name, value) in set {
        let Some(member) = schema.member_schema(name) else {
            return with_segment(path, name, |path| {
                Err(InputError::new(
                    path.clone(),
                    schema,
                    InputErrorKind::UnknownMember,
                ))
            });
        };
        with_segment(path, name, |path| check_value(schemas, member, value, path))?;
    }
    Ok(())
}

fn fits<T: TryFrom<Number>>(
    number: Number,
    error: impl Fn(InputErrorKind) -> Result<(), InputError>,
) -> Result<(), InputError> {
    match T::try_from(number) {
        Ok(_) => Ok(()),
        Err(_) => error(InputErrorKind::InvalidNumber),
    }
}

// Runs `check` with `segment` added to the JSON pointer `path`, then takes the segment off.
fn with_segment(
    path: &mut String,
    segment: &str,
    check: impl FnOnce(&mut String) -> Result<(), InputError>,
) -> Result<(), InputError> {
    let length = path.len();
    path.push('/');
    path.push_str(&segment.replace('~', "~0").replace('/', "~1"));
    let result = check(path);
    path.truncate(length);
    result
}

#[cfg(test)]
mod tests {
    use super::check_input;
    use crate::error::InputErrorKind;
    use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
    use aws_smithy_lang::{Model, ShapeId};
    use aws_smithy_types::{Document, Number};
    use std::path::PathBuf;

    fn schemas() -> RuntimeSchemaSet {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/models/sprockets.json");
        let model = Model::from_json_file(&path).unwrap();
        RuntimeSchemaSet::builder(&model)
            .for_service(&ShapeId::new("smithy.example#Sprockets").unwrap())
            .build()
            .unwrap()
    }

    fn object<const N: usize>(members: [(&str, Document); N]) -> Document {
        Document::Object(
            members
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    fn string(value: &str) -> Document {
        Document::String(value.to_owned())
    }

    fn number(value: u64) -> Document {
        Document::Number(Number::PosInt(value))
    }

    // Checks `input` against the input of `UpdateSprocket`, and returns the error's path and kind.
    fn check(input: Document) -> Result<(), (String, InputErrorKind)> {
        let schemas = schemas();
        let id = ShapeId::new("smithy.example#UpdateSprocket").unwrap();
        let schema = schemas.operation(&id).unwrap().input();
        check_input(&schemas, schema, &input)
            .map_err(|error| (error.path().to_owned(), error.kind().clone()))
    }

    #[test]
    fn an_input_that_matches_the_schema_passes() {
        let input = object([
            ("id", string("1")),
            ("size", number(3)),
            (
                "tags",
                Document::Array(vec![object([("key", string("color"))]), Document::Null]),
            ),
            ("labels", object([("a", string("b"))])),
            ("shape", object([("round", Document::Bool(true))])),
            ("note", Document::Null),
        ]);
        assert_eq!(Ok(()), check(input));
    }

    #[test]
    fn a_value_of_the_wrong_type_fails_with_its_path() {
        let input = object([("id", number(1))]);
        assert_eq!(
            Err(("/id".to_owned(), InputErrorKind::WrongType)),
            check(input)
        );
    }

    #[test]
    fn a_nested_error_has_the_full_path() {
        let tags = Document::Array(vec![
            object([("key", string("color"))]),
            object([("key", number(5))]),
        ]);
        let input = object([("tags", tags)]);
        assert_eq!(
            Err(("/tags/1/key".to_owned(), InputErrorKind::WrongType)),
            check(input)
        );
    }

    #[test]
    fn a_number_out_of_range_fails() {
        let input = object([("size", number(300))]);
        assert_eq!(
            Err(("/size".to_owned(), InputErrorKind::InvalidNumber)),
            check(input)
        );
    }

    #[test]
    fn an_unknown_member_fails() {
        let input = object([("nope", string("x"))]);
        assert_eq!(
            Err(("/nope".to_owned(), InputErrorKind::UnknownMember)),
            check(input)
        );
    }

    #[test]
    fn a_union_with_two_members_fails() {
        let shape = object([("round", Document::Bool(true)), ("sides", number(4))]);
        let input = object([("shape", shape)]);
        assert_eq!(
            Err(("/shape".to_owned(), InputErrorKind::InvalidUnion)),
            check(input)
        );
    }

    #[test]
    fn a_null_value_in_a_map_without_sparse_fails() {
        let input = object([("labels", object([("a", Document::Null)]))]);
        assert_eq!(
            Err(("/labels/a".to_owned(), InputErrorKind::WrongType)),
            check(input)
        );
    }

    #[test]
    fn a_string_for_a_blob_fails() {
        let input = object([("data", string("aGVsbG8="))]);
        assert_eq!(
            Err(("/data".to_owned(), InputErrorKind::Unsupported)),
            check(input)
        );
    }
}
