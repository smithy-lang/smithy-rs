/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Serialization and deserialization interfaces for the Smithy data model.

mod deserializer;
mod serializer;

pub use deserializer::ShapeDeserializer;
pub use serializer::{SerializableStruct, ShapeSerializer};

#[cfg(test)]
mod test {
    use crate::serde::{ShapeDeserializer, ShapeSerializer};
    use crate::{prelude::*, Schema};
    use std::fmt;

    // Mock error type for testing
    #[derive(Debug)]
    struct MockError(String);

    impl fmt::Display for MockError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for MockError {}

    // Mock serializer for testing
    struct MockSerializer {
        output: Vec<String>,
    }

    impl ShapeSerializer for MockSerializer {
        type Output = Vec<String>;
        type Error = MockError;

        fn finish(self) -> Result<Self::Output, Self::Error> {
            Ok(self.output)
        }

        fn write_struct<F>(
            &mut self,
            schema: &dyn Schema,
            write_members: F,
        ) -> Result<(), Self::Error>
        where
            F: FnOnce(&mut Self) -> Result<(), Self::Error>,
        {
            self.output
                .push(format!("struct({})", schema.shape_id().as_str()));
            write_members(self)?;
            self.output.push("end_struct".to_string());
            Ok(())
        }

        fn write_list<F>(
            &mut self,
            schema: &dyn Schema,
            write_elements: F,
        ) -> Result<(), Self::Error>
        where
            F: FnOnce(&mut Self) -> Result<(), Self::Error>,
        {
            self.output
                .push(format!("list({})", schema.shape_id().as_str()));
            write_elements(self)?;
            self.output.push("end_list".to_string());
            Ok(())
        }

        fn write_map<F>(&mut self, schema: &dyn Schema, write_entries: F) -> Result<(), Self::Error>
        where
            F: FnOnce(&mut Self) -> Result<(), Self::Error>,
        {
            self.output
                .push(format!("map({})", schema.shape_id().as_str()));
            write_entries(self)?;
            self.output.push("end_map".to_string());
            Ok(())
        }

        fn write_boolean(&mut self, _schema: &dyn Schema, value: bool) -> Result<(), Self::Error> {
            self.output.push(format!("bool({})", value));
            Ok(())
        }

        fn write_byte(&mut self, _schema: &dyn Schema, value: i8) -> Result<(), Self::Error> {
            self.output.push(format!("byte({})", value));
            Ok(())
        }

        fn write_short(&mut self, _schema: &dyn Schema, value: i16) -> Result<(), Self::Error> {
            self.output.push(format!("short({})", value));
            Ok(())
        }

        fn write_integer(&mut self, _schema: &dyn Schema, value: i32) -> Result<(), Self::Error> {
            self.output.push(format!("int({})", value));
            Ok(())
        }

        fn write_long(&mut self, _schema: &dyn Schema, value: i64) -> Result<(), Self::Error> {
            self.output.push(format!("long({})", value));
            Ok(())
        }

        fn write_float(&mut self, _schema: &dyn Schema, value: f32) -> Result<(), Self::Error> {
            self.output.push(format!("float({})", value));
            Ok(())
        }

        fn write_double(&mut self, _schema: &dyn Schema, value: f64) -> Result<(), Self::Error> {
            self.output.push(format!("double({})", value));
            Ok(())
        }

        fn write_big_integer(
            &mut self,
            _schema: &dyn Schema,
            value: &aws_smithy_types::BigInteger,
        ) -> Result<(), Self::Error> {
            self.output.push(format!("bigint({})", value.as_ref()));
            Ok(())
        }

        fn write_big_decimal(
            &mut self,
            _schema: &dyn Schema,
            value: &aws_smithy_types::BigDecimal,
        ) -> Result<(), Self::Error> {
            self.output.push(format!("bigdec({})", value.as_ref()));
            Ok(())
        }

        fn write_string(&mut self, _schema: &dyn Schema, value: &str) -> Result<(), Self::Error> {
            self.output.push(format!("string({})", value));
            Ok(())
        }

        fn write_blob(
            &mut self,
            _schema: &dyn Schema,
            value: &aws_smithy_types::Blob,
        ) -> Result<(), Self::Error> {
            self.output
                .push(format!("blob({} bytes)", value.as_ref().len()));
            Ok(())
        }

        fn write_timestamp(
            &mut self,
            _schema: &dyn Schema,
            value: &aws_smithy_types::DateTime,
        ) -> Result<(), Self::Error> {
            self.output.push(format!("timestamp({})", value));
            Ok(())
        }

        fn write_document(
            &mut self,
            _schema: &dyn Schema,
            _value: &aws_smithy_types::Document,
        ) -> Result<(), Self::Error> {
            self.output.push("document".to_string());
            Ok(())
        }

        fn write_null(&mut self, _schema: &dyn Schema) -> Result<(), Self::Error> {
            self.output.push("null".to_string());
            Ok(())
        }
    }

    // Mock deserializer for testing
    struct MockDeserializer {
        values: Vec<String>,
        index: usize,
    }

    impl MockDeserializer {
        fn new(values: Vec<String>) -> Self {
            Self { values, index: 0 }
        }
    }

    impl ShapeDeserializer for MockDeserializer {
        type Error = MockError;

        fn read_struct<T, F>(
            &mut self,
            _schema: &dyn Schema,
            state: T,
            mut consumer: F,
        ) -> Result<T, Self::Error>
        where
            F: FnMut(T, &dyn Schema, &mut Self) -> Result<T, Self::Error>,
        {
            // Simulate reading 2 members
            let state = consumer(state, &STRING, self)?;
            let state = consumer(state, &INTEGER, self)?;
            Ok(state)
        }

        fn read_list<T, F>(
            &mut self,
            _schema: &dyn Schema,
            mut state: T,
            mut consumer: F,
        ) -> Result<T, Self::Error>
        where
            F: FnMut(T, &mut Self) -> Result<T, Self::Error>,
        {
            // Simulate reading 3 elements
            for _ in 0..3 {
                state = consumer(state, self)?;
            }
            Ok(state)
        }

        fn read_map<T, F>(
            &mut self,
            _schema: &dyn Schema,
            mut state: T,
            mut consumer: F,
        ) -> Result<T, Self::Error>
        where
            F: FnMut(T, String, &mut Self) -> Result<T, Self::Error>,
        {
            // Simulate reading 2 entries
            state = consumer(state, "key1".to_string(), self)?;
            state = consumer(state, "key2".to_string(), self)?;
            Ok(state)
        }

        fn read_boolean(&mut self, _schema: &dyn Schema) -> Result<bool, Self::Error> {
            Ok(true)
        }

        fn read_byte(&mut self, _schema: &dyn Schema) -> Result<i8, Self::Error> {
            Ok(42)
        }

        fn read_short(&mut self, _schema: &dyn Schema) -> Result<i16, Self::Error> {
            Ok(1000)
        }

        fn read_integer(&mut self, _schema: &dyn Schema) -> Result<i32, Self::Error> {
            Ok(123456)
        }

        fn read_long(&mut self, _schema: &dyn Schema) -> Result<i64, Self::Error> {
            Ok(9876543210)
        }

        fn read_float(&mut self, _schema: &dyn Schema) -> Result<f32, Self::Error> {
            Ok(3.14)
        }

        fn read_double(&mut self, _schema: &dyn Schema) -> Result<f64, Self::Error> {
            Ok(2.71828)
        }

        fn read_big_integer(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::BigInteger, Self::Error> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigInteger::from_str("12345").unwrap())
        }

        fn read_big_decimal(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::BigDecimal, Self::Error> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigDecimal::from_str("123.45").unwrap())
        }

        fn read_string(&mut self, _schema: &dyn Schema) -> Result<String, Self::Error> {
            if self.index < self.values.len() {
                let value = self.values[self.index].clone();
                self.index += 1;
                Ok(value)
            } else {
                Ok("default".to_string())
            }
        }

        fn read_blob(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::Blob, Self::Error> {
            Ok(aws_smithy_types::Blob::new(vec![1, 2, 3, 4]))
        }

        fn read_timestamp(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::DateTime, Self::Error> {
            Ok(aws_smithy_types::DateTime::from_secs(1234567890))
        }

        fn read_document(
            &mut self,
            _schema: &dyn Schema,
        ) -> Result<aws_smithy_types::Document, Self::Error> {
            Ok(aws_smithy_types::Document::Null)
        }

        fn is_null(&self) -> bool {
            false
        }

        fn container_size(&self) -> Option<usize> {
            Some(10)
        }
    }

    #[test]
    fn test_serializer_simple_types() {
        let mut ser = MockSerializer { output: Vec::new() };

        ser.write_boolean(&BOOLEAN, true).unwrap();
        ser.write_integer(&INTEGER, 42).unwrap();
        ser.write_string(&STRING, "hello").unwrap();

        let output = ser.finish().unwrap();
        assert_eq!(output, vec!["bool(true)", "int(42)", "string(hello)"]);
    }

    #[test]
    fn test_serializer_struct() {
        let mut ser = MockSerializer { output: Vec::new() };

        ser.write_struct(&STRING, |s| {
            s.write_string(&STRING, "field1")?;
            s.write_integer(&INTEGER, 123)?;
            Ok(())
        })
        .unwrap();

        let output = ser.finish().unwrap();
        assert_eq!(
            output,
            vec![
                "struct(smithy.api#String)",
                "string(field1)",
                "int(123)",
                "end_struct"
            ]
        );
    }

    #[test]
    fn test_deserializer_simple_types() {
        let mut deser = MockDeserializer::new(vec!["test".to_string()]);

        assert_eq!(deser.read_boolean(&BOOLEAN).unwrap(), true);
        assert_eq!(deser.read_integer(&INTEGER).unwrap(), 123456);
        assert_eq!(deser.read_string(&STRING).unwrap(), "test");
        assert_eq!(deser.container_size(), Some(10));
        assert!(!deser.is_null());
    }

    #[test]
    fn test_deserializer_struct() {
        let mut deser = MockDeserializer::new(vec!["value1".to_string(), "value2".to_string()]);

        let mut fields = Vec::new();
        deser
            .read_struct(&STRING, &mut fields, |fields, _member, d| {
                fields.push(d.read_string(&STRING)?);
                Ok(fields)
            })
            .unwrap();

        assert_eq!(fields, vec!["value1", "value2"]);
    }

    #[test]
    fn test_deserializer_list() {
        let mut deser =
            MockDeserializer::new(vec!["a".to_string(), "b".to_string(), "c".to_string()]);

        let mut elements = Vec::new();
        deser
            .read_list(&STRING, &mut elements, |elements, d| {
                elements.push(d.read_string(&STRING)?);
                Ok(elements)
            })
            .unwrap();

        assert_eq!(elements, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_deserializer_map() {
        let mut deser = MockDeserializer::new(vec!["val1".to_string(), "val2".to_string()]);

        let mut entries = Vec::new();
        deser
            .read_map(&STRING, &mut entries, |entries, key, d| {
                let value = d.read_string(&STRING)?;
                entries.push((key, value));
                Ok(entries)
            })
            .unwrap();

        assert_eq!(
            entries,
            vec![
                ("key1".to_string(), "val1".to_string()),
                ("key2".to_string(), "val2".to_string())
            ]
        );
    }
}
