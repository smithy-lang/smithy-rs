/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::escape::escape_string;

pub struct JsonObjectWriter<'a> {
    json: &'a mut String,
    started: bool,
}

impl<'a> JsonObjectWriter<'a> {
    pub fn new(output: &'a mut String) -> Self {
        output.push('{');
        Self {
            json: output,
            started: false,
        }
    }

    /// Writes a null value with the given `key`.
    pub fn null(&mut self, key: &str) -> &mut Self {
        self.key(key);
        self.json.push_str("null");
        self
    }

    /// Writes the boolean `value` with the given `key`.
    pub fn boolean(&mut self, key: &str, value: bool) -> &mut Self {
        self.key(key);
        self.json.push_str(match value {
            true => "true",
            _ => "false",
        });
        self
    }

    /// Writes a string `value` with the given `key`.
    pub fn string(&mut self, key: &str, value: &str) -> &mut Self {
        self.key(key);
        append_string(&mut self.json, value);
        self
    }

    /// Writes a number `value` (already converted to string) with the given `key`.
    pub fn number(&mut self, key: &str, value: &str) -> &mut Self {
        self.key(key);
        self.json.push_str(value);
        self
    }

    /// Starts an array with the given `key`.
    pub fn start_array(&mut self, key: &str) -> JsonArrayWriter {
        self.key(key);
        JsonArrayWriter::new(&mut self.json)
    }

    /// Starts an object with the given `key`.
    pub fn start_object(&mut self, key: &str) -> JsonObjectWriter {
        self.key(key);
        JsonObjectWriter::new(&mut self.json)
    }

    /// Finishes the object.
    pub fn finish(self) {
        self.json.push('}');
    }

    fn key(&mut self, key: &str) {
        if self.started {
            self.json.push(',');
        }
        self.started = true;

        self.json.push('"');
        self.json.push_str(&escape_string(key));
        self.json.push_str("\":");
    }
}

pub struct JsonArrayWriter<'a> {
    json: &'a mut String,
    started: bool,
}

impl<'a> JsonArrayWriter<'a> {
    pub fn new(output: &'a mut String) -> Self {
        output.push('[');
        Self {
            json: output,
            started: false,
        }
    }

    /// Writes a null value to the array.
    pub fn null(&mut self) -> &mut Self {
        self.comma_delimit();
        self.json.push_str("null");
        self
    }

    /// Writes the boolean `value` to the array.
    pub fn boolean(&mut self, value: bool) -> &mut Self {
        self.comma_delimit();
        self.json.push_str(match value {
            true => "true",
            _ => "false",
        });
        self
    }

    /// Writes a string to the array.
    pub fn string(&mut self, value: &str) -> &mut Self {
        self.comma_delimit();
        append_string(&mut self.json, value);
        self
    }

    /// Writes a number `value` (already converted to string).
    pub fn number(&mut self, value: &str) -> &mut Self {
        self.comma_delimit();
        self.json.push_str(value);
        self
    }

    /// Starts a nested array inside of the array.
    pub fn start_array(&mut self) -> JsonArrayWriter {
        self.comma_delimit();
        JsonArrayWriter::new(&mut self.json)
    }

    /// Starts a nested object inside of the array.
    pub fn start_object(&mut self) -> JsonObjectWriter {
        self.comma_delimit();
        JsonObjectWriter::new(&mut self.json)
    }

    /// Finishes the array.
    pub fn finish(self) {
        self.json.push(']');
    }

    fn comma_delimit(&mut self) {
        if self.started {
            self.json.push(',');
        }
        self.started = true;
    }
}

fn append_string(json: &mut String, value: &str) {
    json.push('"');
    json.push_str(&escape_string(value));
    json.push('"');
}

#[cfg(test)]
mod tests {
    use super::{JsonArrayWriter, JsonObjectWriter};

    #[test]
    fn empty() {
        let mut output = String::new();
        JsonObjectWriter::new(&mut output).finish();
        assert_eq!("{}", &output);

        let mut output = String::new();
        JsonArrayWriter::new(&mut output).finish();
        assert_eq!("[]", &output);
    }

    #[test]
    fn object_inside_array() {
        let mut output = String::new();
        let mut array = JsonArrayWriter::new(&mut output);
        array.start_object().finish();
        array.start_object().finish();
        array.start_object().finish();
        array.finish();
        assert_eq!("[{},{},{}]", &output);
    }

    #[test]
    fn object_inside_object() {
        let mut output = String::new();
        let mut obj_1 = JsonObjectWriter::new(&mut output);

        let mut obj_2 = obj_1.start_object("nested");
        obj_2.string("test", "test");
        obj_2.finish();

        obj_1.finish();
        assert_eq!(r#"{"nested":{"test":"test"}}"#, &output);
    }

    #[test]
    fn array_inside_object() {
        let mut output = String::new();
        let mut object = JsonObjectWriter::new(&mut output);
        object.start_array("foo").finish();
        object.start_array("ba\nr").finish();
        object.finish();
        assert_eq!(r#"{"foo":[],"ba\nr":[]}"#, &output);
    }

    #[test]
    fn array_inside_array() {
        let mut output = String::new();

        let mut arr_1 = JsonArrayWriter::new(&mut output);

        let mut arr_2 = arr_1.start_array();
        arr_2.number("5");
        arr_2.finish();

        arr_1.start_array().finish();
        arr_1.finish();

        assert_eq!("[[5],[]]", &output);
    }

    #[test]
    fn object() {
        let mut output = String::new();
        let mut object = JsonObjectWriter::new(&mut output);
        object.string("some_string", "some\nstring\nvalue");
        object.number("some_number", "3.5");

        let mut array = object.start_array("some_mixed_array");
        array.string("1").number("2");
        array.finish();

        object.finish();

        assert_eq!(
            r#"{"some_string":"some\nstring\nvalue","some_number":3.5,"some_mixed_array":["1",2]}"#,
            &output
        );
    }
}
