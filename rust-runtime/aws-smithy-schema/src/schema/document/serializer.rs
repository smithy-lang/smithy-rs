/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentShapeSerializer`] — a [`ShapeSerializer`] implementation that
//! builds up a [`Document`] tree from a typed Smithy shape.
//!
//! A [`ShapeSerializer`] receives a sequence of `write_*(schema, value)`
//! calls driven either by `value.serialize_members(self)` (for structures)
//! or by a closure passed to `write_list`/`write_map`. The serializer does
//! not see the shape's static type; it only sees the schema and the value
//! at each call site.
//!
//! [`DocumentShapeSerializer`] turns each such call into a [`Document`]
//! node and stitches them into a tree using a small frame stack:
//!
//! - **No frame on the stack** — the next `write_*` call sets the root
//!   document. The common entry point is [`Document::from_struct`], which
//!   immediately calls `write_struct` and produces a structure-typed root.
//!   A bare `write_string` (etc.) at the top level produces a scalar root,
//!   which is also valid (e.g. a payload that's just `"foo"`).
//! - **Struct frame** — each `write_*(member_schema, value)` insertion
//!   uses `member_schema.member_name()` as the map key.
//! - **List frame** — each `write_*` value is appended to the list.
//! - **Map frame** — calls alternate `key`/`value`. The first call in a
//!   map frame must produce a [`String`] document (the key); the next
//!   call's value is bound to that key.
//!
//! When a frame's body completes, the serializer pops the frame, wraps it
//! in the appropriate [`Document`] variant, and commits the wrapper to
//! the parent frame (or to the root slot if the stack is now empty).

use std::collections::HashMap;

use aws_smithy_types::{BigDecimal, BigInteger, DateTime};

use super::{Document, DocumentInner};
use crate::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use crate::{Schema, ShapeId};

/// Builds a [`Document`] tree from a typed shape via the
/// [`ShapeSerializer`] interface.
///
/// See the module-level documentation for an overview.
///
/// # Example
///
/// Use [`Document::from_struct`] for the common case of converting a
/// `SerializableStruct` into a `Document`:
///
/// ```ignore
/// use aws_smithy_schema::document::Document;
///
/// let doc = Document::from_struct(MyStruct::SCHEMA, &my_struct)?;
/// ```
///
/// Direct construction is useful when round-tripping pieces of a tree
/// outside the standard entry point:
///
/// ```ignore
/// use aws_smithy_schema::document::DocumentShapeSerializer;
/// use aws_smithy_schema::serde::ShapeSerializer;
///
/// let mut ser = DocumentShapeSerializer::new();
/// ser.write_string(&aws_smithy_schema::prelude::STRING, "hello")?;
/// let doc = ser.finish()?;
/// ```
#[derive(Debug, Default)]
pub struct DocumentShapeSerializer {
    /// Stack of in-progress containers. Top is the active write target.
    stack: Vec<Frame>,
    /// Holds the root document once it is committed (i.e. once a write_*
    /// call returns with an empty stack).
    finished: Option<Document>,
}

/// An in-progress container on the serializer's frame stack.
#[derive(Debug)]
enum Frame {
    Struct {
        members: HashMap<String, Document>,
        discriminator: Option<ShapeId>,
    },
    List(Vec<Document>),
    Map {
        entries: HashMap<String, Document>,
        /// `Some(k)` after a key has been written and we're awaiting the
        /// matching value; `None` when we are at an entry boundary
        /// (next write becomes the next key).
        pending_key: Option<String>,
    },
}

impl DocumentShapeSerializer {
    /// Creates a fresh serializer with an empty frame stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes the serializer and returns the constructed [`Document`].
    ///
    /// Returns an error if no value has been written yet, if any frame
    /// is still open (caller forgot a closing callback), or if the
    /// serializer was driven into a malformed state.
    pub fn finish(self) -> Result<Document, SerdeError> {
        if !self.stack.is_empty() {
            return Err(SerdeError::custom(format!(
                "DocumentShapeSerializer::finish called with {} unfinished container(s) on the stack",
                self.stack.len()
            )));
        }
        self.finished.ok_or_else(|| {
            SerdeError::custom(
                "DocumentShapeSerializer::finish called before any value was written",
            )
        })
    }

    /// Routes a constructed [`Document`] into the active frame, or commits
    /// it as the root if the stack is empty.
    fn commit_value(&mut self, schema: &Schema, value: Document) -> Result<(), SerdeError> {
        match self.stack.last_mut() {
            None => {
                if self.finished.is_some() {
                    return Err(SerdeError::custom(
                        "DocumentShapeSerializer received multiple top-level writes",
                    ));
                }
                self.finished = Some(value);
                Ok(())
            }
            Some(Frame::Struct { members, .. }) => {
                let name = schema.member_name().ok_or_else(|| {
                    SerdeError::custom(format!(
                        "writing a struct member requires a member schema with member_name set; got schema for {}",
                        schema.shape_id().as_str()
                    ))
                })?;
                members.insert(name.to_string(), value);
                Ok(())
            }
            Some(Frame::List(items)) => {
                items.push(value);
                Ok(())
            }
            Some(Frame::Map {
                entries,
                pending_key,
            }) => {
                if let Some(key) = pending_key.take() {
                    entries.insert(key, value);
                } else {
                    // First write in an entry must be the key, and
                    // map keys are always strings in the Smithy data model.
                    let key = match value.inner() {
                        DocumentInner::String(s) => s.clone(),
                        other => {
                            return Err(SerdeError::custom(format!(
                                "map key must be a String document; got {}",
                                shape_kind_name(other),
                            )))
                        }
                    };
                    *pending_key = Some(key);
                }
                Ok(())
            }
        }
    }
}

/// Human-readable name for a [`DocumentInner`] variant, used in error
/// messages for type-mismatch diagnostics.
fn shape_kind_name(inner: &DocumentInner) -> &'static str {
    match inner {
        DocumentInner::Null => "null",
        DocumentInner::Boolean(_) => "boolean",
        DocumentInner::Number(_) => "number",
        DocumentInner::BigInteger(_) => "bigInteger",
        DocumentInner::BigDecimal(_) => "bigDecimal",
        DocumentInner::String(_) => "string",
        DocumentInner::Blob(_) => "blob",
        DocumentInner::Timestamp(_) => "timestamp",
        DocumentInner::List(_) => "list",
        DocumentInner::Map(_) => "map",
    }
}

impl ShapeSerializer for DocumentShapeSerializer {
    fn write_struct(
        &mut self,
        schema: &Schema,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        self.stack.push(Frame::Struct {
            members: HashMap::new(),
            discriminator: Some(*schema.shape_id()),
        });
        value.serialize_members(self)?;
        let frame = self.stack.pop().expect("frame just pushed");
        let (members, discriminator) = match frame {
            Frame::Struct {
                members,
                discriminator,
            } => (members, discriminator),
            _ => {
                return Err(SerdeError::custom(
                    "DocumentShapeSerializer struct frame replaced by a different frame kind during serialization",
                ));
            }
        };
        let mut doc = Document::map(members);
        if let Some(id) = discriminator {
            doc = doc.with_discriminator(id);
        }
        self.commit_value(schema, doc)
    }

    fn write_list(
        &mut self,
        schema: &Schema,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.stack.push(Frame::List(Vec::new()));
        write_elements(self)?;
        let frame = self.stack.pop().expect("frame just pushed");
        let items = match frame {
            Frame::List(items) => items,
            _ => {
                return Err(SerdeError::custom(
                    "DocumentShapeSerializer list frame replaced by a different frame kind during serialization",
                ));
            }
        };
        self.commit_value(schema, Document::list(items))
    }

    fn write_map(
        &mut self,
        schema: &Schema,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.stack.push(Frame::Map {
            entries: HashMap::new(),
            pending_key: None,
        });
        write_entries(self)?;
        let frame = self.stack.pop().expect("frame just pushed");
        let (entries, pending_key) = match frame {
            Frame::Map {
                entries,
                pending_key,
            } => (entries, pending_key),
            _ => {
                return Err(SerdeError::custom(
                    "DocumentShapeSerializer map frame replaced by a different frame kind during serialization",
                ));
            }
        };
        if pending_key.is_some() {
            return Err(SerdeError::custom(
                "map ended with an unmatched key (key written without a matching value)",
            ));
        }
        self.commit_value(schema, Document::map(entries))
    }

    fn write_boolean(&mut self, schema: &Schema, value: bool) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::boolean(value))
    }

    fn write_byte(&mut self, schema: &Schema, value: i8) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::byte(value))
    }

    fn write_short(&mut self, schema: &Schema, value: i16) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::short(value))
    }

    fn write_integer(&mut self, schema: &Schema, value: i32) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::integer(value))
    }

    fn write_long(&mut self, schema: &Schema, value: i64) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::long(value))
    }

    fn write_float(&mut self, schema: &Schema, value: f32) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::float(value))
    }

    fn write_double(&mut self, schema: &Schema, value: f64) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::double(value))
    }

    fn write_big_integer(&mut self, schema: &Schema, value: &BigInteger) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::big_integer(value.clone()))
    }

    fn write_big_decimal(&mut self, schema: &Schema, value: &BigDecimal) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::big_decimal(value.clone()))
    }

    fn write_string(&mut self, schema: &Schema, value: &str) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::string(value))
    }

    fn write_blob(&mut self, schema: &Schema, value: &[u8]) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::blob(value.to_vec()))
    }

    fn write_timestamp(&mut self, schema: &Schema, value: &DateTime) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::timestamp(*value))
    }

    fn write_document(&mut self, schema: &Schema, value: &Document) -> Result<(), SerdeError> {
        self.commit_value(schema, value.clone())
    }

    fn write_null(&mut self, schema: &Schema) -> Result<(), SerdeError> {
        self.commit_value(schema, Document::null())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::serde::{SerdeError, SerializableStruct, ShapeSerializer};
    use crate::{prelude, shape_id, Schema, ShapeId, ShapeType};

    // -- Test schemas ----------------------------------------------------

    const PERSON_ID: ShapeId = shape_id!("smithy.example", "Person");
    const PERSON_NAME_ID: ShapeId = shape_id!("smithy.example", "Person", "name");
    const PERSON_AGE_ID: ShapeId = shape_id!("smithy.example", "Person", "age");

    static PERSON_NAME_MEMBER: Schema =
        Schema::new_member(PERSON_NAME_ID, ShapeType::String, "name", 0);
    static PERSON_AGE_MEMBER: Schema =
        Schema::new_member(PERSON_AGE_ID, ShapeType::Integer, "age", 1);
    static PERSON_SCHEMA: Schema = Schema::new_struct(
        PERSON_ID,
        ShapeType::Structure,
        &[&PERSON_NAME_MEMBER, &PERSON_AGE_MEMBER],
    );

    /// `struct Person { name: String, age: i32 }`.
    struct Person {
        name: String,
        age: i32,
    }

    impl SerializableStruct for Person {
        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            ser.write_string(&PERSON_NAME_MEMBER, &self.name)?;
            ser.write_integer(&PERSON_AGE_MEMBER, self.age)?;
            Ok(())
        }
    }

    // -- Top-level scalar -----------------------------------------------

    #[test]
    fn top_level_scalar_string_becomes_root() {
        let mut ser = DocumentShapeSerializer::new();
        ser.write_string(&prelude::STRING, "hello").unwrap();
        let doc = ser.finish().unwrap();
        assert_eq!(doc.as_string(), Some("hello"));
        assert!(doc.discriminator().is_none());
    }

    #[test]
    fn top_level_scalar_integer_becomes_root() {
        let mut ser = DocumentShapeSerializer::new();
        ser.write_integer(&prelude::INTEGER, 42).unwrap();
        let doc = ser.finish().unwrap();
        assert_eq!(doc.as_integer().unwrap(), 42);
    }

    #[test]
    fn top_level_null_becomes_root() {
        let mut ser = DocumentShapeSerializer::new();
        ser.write_null(&prelude::STRING).unwrap();
        let doc = ser.finish().unwrap();
        assert!(matches!(doc.inner(), DocumentInner::Null));
    }

    // -- Struct ----------------------------------------------------------

    #[test]
    fn write_struct_produces_map_with_discriminator() {
        let mut ser = DocumentShapeSerializer::new();
        ser.write_struct(
            &PERSON_SCHEMA,
            &Person {
                name: "Iago".into(),
                age: 7,
            },
        )
        .unwrap();
        let doc = ser.finish().unwrap();

        assert_eq!(
            doc.discriminator().map(|id| id.as_str()),
            Some("smithy.example#Person")
        );
        let map = doc.as_map().unwrap();
        assert_eq!(map.get("name").and_then(Document::as_string), Some("Iago"));
        assert_eq!(map.get("age").and_then(|d| d.as_integer().ok()), Some(7));
    }

    // -- List ------------------------------------------------------------

    #[test]
    fn write_list_of_strings() {
        let mut ser = DocumentShapeSerializer::new();
        let values = ["a", "b", "c"];
        ser.write_list(&prelude::DOCUMENT, &|inner| {
            for v in &values {
                inner.write_string(&prelude::STRING, v)?;
            }
            Ok(())
        })
        .unwrap();
        let doc = ser.finish().unwrap();
        let items = doc.as_list().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_string(), Some("a"));
        assert_eq!(items[2].as_string(), Some("c"));
    }

    #[test]
    fn write_sparse_list_with_nulls() {
        let mut ser = DocumentShapeSerializer::new();
        ser.write_list(&prelude::DOCUMENT, &|inner| {
            inner.write_string(&prelude::STRING, "first")?;
            inner.write_null(&prelude::STRING)?;
            inner.write_string(&prelude::STRING, "third")?;
            Ok(())
        })
        .unwrap();
        let doc = ser.finish().unwrap();
        let items = doc.as_list().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_string(), Some("first"));
        assert!(matches!(items[1].inner(), DocumentInner::Null));
        assert_eq!(items[2].as_string(), Some("third"));
    }

    // -- Map -------------------------------------------------------------

    #[test]
    fn write_map_of_strings() {
        let mut ser = DocumentShapeSerializer::new();
        ser.write_map(&prelude::DOCUMENT, &|inner| {
            inner.write_string(&prelude::STRING, "key1")?;
            inner.write_string(&prelude::STRING, "value1")?;
            inner.write_string(&prelude::STRING, "key2")?;
            inner.write_string(&prelude::STRING, "value2")?;
            Ok(())
        })
        .unwrap();
        let doc = ser.finish().unwrap();
        let map = doc.as_map().unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("key1").and_then(Document::as_string),
            Some("value1")
        );
        assert_eq!(
            map.get("key2").and_then(Document::as_string),
            Some("value2")
        );
    }

    #[test]
    fn write_map_with_non_string_key_errors() {
        let mut ser = DocumentShapeSerializer::new();
        let result = ser.write_map(&prelude::DOCUMENT, &|inner| {
            inner.write_integer(&prelude::INTEGER, 1)?;
            inner.write_string(&prelude::STRING, "value")?;
            Ok(())
        });
        let err = result.expect_err("non-string map key should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("map key must be a String document"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn write_map_with_unmatched_key_errors() {
        let mut ser = DocumentShapeSerializer::new();
        let result = ser.write_map(&prelude::DOCUMENT, &|inner| {
            inner.write_string(&prelude::STRING, "lonely-key")?;
            Ok(())
        });
        let err = result.expect_err("trailing key without value should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("unmatched key"),
            "unexpected error message: {msg}"
        );
    }

    // -- Nested aggregates ----------------------------------------------

    #[test]
    fn nested_struct_in_map_round_trips() {
        // map<String, Person> with a single entry
        let mut ser = DocumentShapeSerializer::new();
        ser.write_map(&prelude::DOCUMENT, &|inner| {
            inner.write_string(&prelude::STRING, "owner")?;
            inner.write_struct(
                &PERSON_SCHEMA,
                &Person {
                    name: "Alex".into(),
                    age: 30,
                },
            )?;
            Ok(())
        })
        .unwrap();
        let doc = ser.finish().unwrap();
        let map = doc.as_map().unwrap();
        let nested = map.get("owner").unwrap();
        assert_eq!(
            nested.discriminator().map(|id| id.as_str()),
            Some("smithy.example#Person")
        );
        let nested_members = nested.as_map().unwrap();
        assert_eq!(
            nested_members.get("name").and_then(Document::as_string),
            Some("Alex")
        );
    }

    // -- write_document round-trips a Document ---------------------------

    #[test]
    fn write_document_commits_value_directly() {
        let nested = Document::map(HashMap::from([(
            "foo".to_string(),
            Document::string("bar"),
        )]));
        let mut ser = DocumentShapeSerializer::new();
        ser.write_document(&prelude::DOCUMENT, &nested).unwrap();
        let doc = ser.finish().unwrap();
        assert_eq!(doc.member("foo").and_then(Document::as_string), Some("bar"));
    }

    // -- Finish error paths ---------------------------------------------

    #[test]
    fn finish_with_no_writes_errors() {
        let ser = DocumentShapeSerializer::new();
        let err = ser.finish().expect_err("empty serializer should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("before any value was written"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn double_top_level_write_errors() {
        let mut ser = DocumentShapeSerializer::new();
        ser.write_string(&prelude::STRING, "first").unwrap();
        let err = ser
            .write_string(&prelude::STRING, "second")
            .expect_err("second top-level write should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("multiple top-level writes"),
            "unexpected error message: {msg}"
        );
    }
}
