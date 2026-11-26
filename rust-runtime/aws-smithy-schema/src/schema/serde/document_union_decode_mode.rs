/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`DocumentUnionDecodeMode`], which selects how a generated client union reads a member that
//! holds documents.

use super::{SerdeError, ShapeDeserializer};
use crate::Schema;
use aws_smithy_types::config_bag::{Storable, StoreReplace};
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document, DocumentV2};
use std::collections::HashMap;

/// Selects how a generated client union reads a member whose value holds documents.
///
/// A generated client union has two variants for such a member: the original one, such as
/// `Json(Document)`, and an additive one, such as `JsonV2(DocumentV2)`. Both serialize as the
/// same modeled member. A response reads the original variant unless the caller selects
/// [`V2`](Self::V2) for the operation.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DocumentUnionDecodeMode {
    /// Reads the original variant, such as `Json(Document)`.
    ///
    /// This is the default, and it is what clients read before `DocumentV2` existed.
    #[default]
    V1,
    /// Reads the `DocumentV2` variant, such as `JsonV2(DocumentV2)`, for every such member,
    /// including a value that a `Document` could hold.
    V2,
}

impl Storable for DocumentUnionDecodeMode {
    type Storer = StoreReplace<Self>;
}

impl DocumentUnionDecodeMode {
    /// Calls `read` with a deserializer whose values report this mode through
    /// [`ShapeDeserializer::document_union_decode_mode`].
    ///
    /// For [`V1`](Self::V1), `read` gets `deserializer` itself, so the read is exactly the read
    /// without a mode. For another mode, `read` gets a wrapper that reports the mode and forwards
    /// every read to `deserializer`. The wrapper also wraps each deserializer that a structure,
    /// list, or map read passes to its consumer, so every value that `read` reaches, at any
    /// depth, reports the same mode. Deserializers inside `deserializer` don't need to know about
    /// the mode.
    pub fn read_with<T>(
        self,
        deserializer: &mut dyn ShapeDeserializer,
        read: impl FnOnce(&mut dyn ShapeDeserializer) -> T,
    ) -> T {
        match self {
            Self::V1 => read(deserializer),
            mode => read(&mut ModeDeserializer {
                inner: deserializer,
                mode,
            }),
        }
    }
}

/// Reports `mode` for itself and for every deserializer its aggregate reads pass to a consumer,
/// and forwards everything else to `inner`.
struct ModeDeserializer<'a> {
    inner: &'a mut dyn ShapeDeserializer,
    mode: DocumentUnionDecodeMode,
}

impl ShapeDeserializer for ModeDeserializer<'_> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        state: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let mode = self.mode;
        self.inner.read_struct(schema, &mut |member, inner| {
            state(member, &mut ModeDeserializer { inner, mode })
        })
    }

    fn read_list(
        &mut self,
        schema: &Schema<'_>,
        state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let mode = self.mode;
        self.inner.read_list(schema, &mut |inner| {
            state(&mut ModeDeserializer { inner, mode })
        })
    }

    fn read_map(
        &mut self,
        schema: &Schema<'_>,
        state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let mode = self.mode;
        self.inner.read_map(schema, &mut |key, inner| {
            state(key, &mut ModeDeserializer { inner, mode })
        })
    }

    fn read_boolean(&mut self, schema: &Schema<'_>) -> Result<bool, SerdeError> {
        self.inner.read_boolean(schema)
    }

    fn read_byte(&mut self, schema: &Schema<'_>) -> Result<i8, SerdeError> {
        self.inner.read_byte(schema)
    }

    fn read_short(&mut self, schema: &Schema<'_>) -> Result<i16, SerdeError> {
        self.inner.read_short(schema)
    }

    fn read_integer(&mut self, schema: &Schema<'_>) -> Result<i32, SerdeError> {
        self.inner.read_integer(schema)
    }

    fn read_long(&mut self, schema: &Schema<'_>) -> Result<i64, SerdeError> {
        self.inner.read_long(schema)
    }

    fn read_float(&mut self, schema: &Schema<'_>) -> Result<f32, SerdeError> {
        self.inner.read_float(schema)
    }

    fn read_double(&mut self, schema: &Schema<'_>) -> Result<f64, SerdeError> {
        self.inner.read_double(schema)
    }

    fn read_big_integer(&mut self, schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        self.inner.read_big_integer(schema)
    }

    fn read_big_decimal(&mut self, schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        self.inner.read_big_decimal(schema)
    }

    fn read_string(&mut self, schema: &Schema<'_>) -> Result<String, SerdeError> {
        self.inner.read_string(schema)
    }

    fn read_blob(&mut self, schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        self.inner.read_blob(schema)
    }

    fn read_timestamp(&mut self, schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        self.inner.read_timestamp(schema)
    }

    fn read_document(&mut self, schema: &Schema<'_>) -> Result<Document, SerdeError> {
        self.inner.read_document(schema)
    }

    fn read_document_v2(&mut self, schema: &Schema<'_>) -> Result<DocumentV2, SerdeError> {
        self.inner.read_document_v2(schema)
    }

    fn is_null(&self) -> bool {
        self.inner.is_null()
    }

    fn read_null(&mut self) -> Result<(), SerdeError> {
        self.inner.read_null()
    }

    fn skip_value(&mut self) -> Result<(), SerdeError> {
        self.inner.skip_value()
    }

    fn container_size(&self) -> Option<usize> {
        self.inner.container_size()
    }

    // The collection helpers hold no unions, so they go straight to the inner deserializer and
    // keep its specialized implementations.

    fn read_string_list(&mut self, schema: &Schema<'_>) -> Result<Vec<String>, SerdeError> {
        self.inner.read_string_list(schema)
    }

    fn read_blob_list(&mut self, schema: &Schema<'_>) -> Result<Vec<Blob>, SerdeError> {
        self.inner.read_blob_list(schema)
    }

    fn read_integer_list(&mut self, schema: &Schema<'_>) -> Result<Vec<i32>, SerdeError> {
        self.inner.read_integer_list(schema)
    }

    fn read_long_list(&mut self, schema: &Schema<'_>) -> Result<Vec<i64>, SerdeError> {
        self.inner.read_long_list(schema)
    }

    fn read_string_string_map(
        &mut self,
        schema: &Schema<'_>,
    ) -> Result<HashMap<String, String>, SerdeError> {
        self.inner.read_string_string_map(schema)
    }

    fn document_union_decode_mode(&self) -> DocumentUnionDecodeMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentV2ShapeDeserializer;
    use crate::prelude::DOCUMENT;
    use std::str::FromStr;

    /// A codec that hands itself to the consumer of each aggregate read until `depth` runs out,
    /// cycling through structure, list, and map reads, and holds `null` everywhere.
    struct Nested {
        depth: usize,
    }

    impl ShapeDeserializer for Nested {
        fn read_struct(
            &mut self,
            _: &Schema<'_>,
            state: &mut dyn FnMut(
                &Schema<'_>,
                &mut dyn ShapeDeserializer,
            ) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            self.depth -= 1;
            state(&DOCUMENT, self)
        }
        fn read_list(
            &mut self,
            _: &Schema<'_>,
            state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            self.depth -= 1;
            state(self)
        }
        fn read_map(
            &mut self,
            _: &Schema<'_>,
            state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            self.depth -= 1;
            state("key".into(), self)
        }
        fn read_boolean(&mut self, _: &Schema<'_>) -> Result<bool, SerdeError> {
            Ok(true)
        }
        fn read_byte(&mut self, _: &Schema<'_>) -> Result<i8, SerdeError> {
            Ok(0)
        }
        fn read_short(&mut self, _: &Schema<'_>) -> Result<i16, SerdeError> {
            Ok(0)
        }
        fn read_integer(&mut self, _: &Schema<'_>) -> Result<i32, SerdeError> {
            Ok(0)
        }
        fn read_long(&mut self, _: &Schema<'_>) -> Result<i64, SerdeError> {
            Ok(0)
        }
        fn read_float(&mut self, _: &Schema<'_>) -> Result<f32, SerdeError> {
            Ok(0.0)
        }
        fn read_double(&mut self, _: &Schema<'_>) -> Result<f64, SerdeError> {
            Ok(0.0)
        }
        fn read_big_integer(&mut self, _: &Schema<'_>) -> Result<BigInteger, SerdeError> {
            Ok(BigInteger::from_str("0").unwrap())
        }
        fn read_big_decimal(&mut self, _: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
            Ok(BigDecimal::from_str("0").unwrap())
        }
        fn read_string(&mut self, _: &Schema<'_>) -> Result<String, SerdeError> {
            Ok(String::new())
        }
        fn read_blob(&mut self, _: &Schema<'_>) -> Result<Blob, SerdeError> {
            Ok(Blob::new(""))
        }
        fn read_timestamp(&mut self, _: &Schema<'_>) -> Result<DateTime, SerdeError> {
            Ok(DateTime::from_secs(0))
        }
        fn read_document(&mut self, _: &Schema<'_>) -> Result<Document, SerdeError> {
            Ok(Document::Null)
        }
        fn is_null(&self) -> bool {
            true
        }
        fn container_size(&self) -> Option<usize> {
            Some(self.depth)
        }
    }

    /// Reads `deser` as nested aggregates until `depth` reads remain, recording the mode each
    /// value reports, outermost first.
    fn modes(deser: &mut dyn ShapeDeserializer, depth: usize) -> Vec<DocumentUnionDecodeMode> {
        let mut out = vec![deser.document_union_decode_mode()];
        if depth == 0 {
            return out;
        }
        match depth % 3 {
            0 => deser.read_struct(&DOCUMENT, &mut |_, d| {
                out.extend(modes(d, depth - 1));
                Ok(())
            }),
            1 => deser.read_list(&DOCUMENT, &mut |d| {
                out.extend(modes(d, depth - 1));
                Ok(())
            }),
            _ => deser.read_map(&DOCUMENT, &mut |_, d| {
                out.extend(modes(d, depth - 1));
                Ok(())
            }),
        }
        .unwrap();
        out
    }

    #[test]
    fn the_default_is_v1() {
        assert_eq!(
            DocumentUnionDecodeMode::default(),
            DocumentUnionDecodeMode::V1
        );
        assert_eq!(
            Nested { depth: 0 }.document_union_decode_mode(),
            DocumentUnionDecodeMode::V1
        );
    }

    #[test]
    fn v1_reads_through_the_deserializer_unchanged() {
        let mut deser = Nested { depth: 6 };
        let modes = DocumentUnionDecodeMode::V1.read_with(&mut deser, |d| modes(d, 6));
        assert_eq!(modes, vec![DocumentUnionDecodeMode::V1; 7]);
    }

    #[test]
    fn v2_reaches_every_nested_structure_list_and_map_value() {
        let mut deser = Nested { depth: 6 };
        let modes = DocumentUnionDecodeMode::V2.read_with(&mut deser, |d| modes(d, 6));
        assert_eq!(modes, vec![DocumentUnionDecodeMode::V2; 7]);
        // Every read went to the codec.
        assert_eq!(deser.depth, 0);
    }

    #[test]
    fn reads_are_forwarded_to_the_inner_deserializer() {
        DocumentUnionDecodeMode::V2.read_with(&mut Nested { depth: 3 }, |d| {
            assert!(d.is_null());
            assert_eq!(d.container_size(), Some(3));
            assert!(d.read_boolean(&DOCUMENT).unwrap());
            assert_eq!(d.read_document(&DOCUMENT).unwrap(), Document::Null);
            d.read_null().unwrap();
            d.skip_value().unwrap();
        });

        // `read_document_v2` must reach the inner deserializer's own implementation; the trait
        // default would read through `read_document` and lose the big integer.
        let big =
            DocumentV2::BigInteger(BigInteger::from_str("123456789012345678901234567890").unwrap());
        let read = DocumentUnionDecodeMode::V2
            .read_with(&mut DocumentV2ShapeDeserializer::new(&big), |d| {
                d.read_document_v2(&DOCUMENT).unwrap()
            });
        assert_eq!(read, big);
    }

    #[test]
    fn read_with_returns_the_value_read() {
        let value = DocumentUnionDecodeMode::V2
            .read_with(&mut Nested { depth: 0 }, |d| d.read_integer(&DOCUMENT));
        assert_eq!(value.unwrap(), 0);
    }
}
