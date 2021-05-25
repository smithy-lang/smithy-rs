/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::escape::escape_string;
use smithy_types::instant::Format;
use smithy_types::{Instant, Number};

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

    /// Writes a string `value` with the given `key` without escaping it.
    pub fn string_unchecked(&mut self, key: &str, value: &str) -> &mut Self {
        self.key(key);
        append_string_unchecked(&mut self.json, value);
        self
    }

    /// Writes a number `value` with the given `key`.
    pub fn number(&mut self, key: &str, value: Number) -> &mut Self {
        self.key(key);
        append_number(&mut self.json, value);
        self
    }

    /// Writes an Instant `value` with the given `key` and `format`.
    pub fn instant(&mut self, key: &str, instant: &Instant, format: Format) -> &mut Self {
        self.key(key);
        append_instant(&mut self.json, instant, format);
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

    /// Writes a string `value` to the array without escaping it.
    pub fn string_unchecked(&mut self, value: &str) -> &mut Self {
        self.comma_delimit();
        append_string_unchecked(&mut self.json, value);
        self
    }

    /// Writes a number `value` to the array.
    pub fn number(&mut self, value: Number) -> &mut Self {
        self.comma_delimit();
        append_number(&mut self.json, value);
        self
    }

    /// Writes an Instant `value` using `format` to the array.
    pub fn instant(&mut self, instant: &Instant, format: Format) -> &mut Self {
        self.comma_delimit();
        append_instant(&mut self.json, instant, format);
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
    append_string_unchecked(json, &escape_string(value));
}

fn append_string_unchecked(json: &mut String, value: &str) {
    json.push('"');
    json.push_str(value);
    json.push('"');
}

fn append_instant(json: &mut String, value: &Instant, format: Format) {
    let formatted = value.fmt(format);
    match format {
        Format::EpochSeconds => json.push_str(&formatted),
        _ => append_string(json, &formatted),
    }
}

fn append_number(json: &mut String, value: Number) {
    match value {
        Number::PosInt(value) => {
            // itoa::Buffer is a fixed-size stack allocation, so this is cheap
            json.push_str(itoa::Buffer::new().format(value));
        }
        Number::NegInt(value) => {
            json.push_str(itoa::Buffer::new().format(value));
        }
        Number::Float(value) => {
            // If the value is NaN, Infinity, or -Infinity
            if value.is_nan() || value.is_infinite() {
                json.push_str("null");
            } else {
                // ryu::Buffer is a fixed-size stack allocation, so this is cheap
                json.push_str(ryu::Buffer::new().format_finite(value));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{JsonArrayWriter, JsonObjectWriter};
    use crate::serialize::{append_number, append_string_unchecked};
    use proptest::proptest;
    use smithy_types::instant::Format;
    use smithy_types::{Instant, Number};

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
        arr_2.number(Number::PosInt(5));
        arr_2.finish();

        arr_1.start_array().finish();
        arr_1.finish();

        assert_eq!("[[5],[]]", &output);
    }

    #[test]
    fn object() {
        let mut output = String::new();
        let mut object = JsonObjectWriter::new(&mut output);
        object.boolean("true_val", true);
        object.boolean("false_val", false);
        object.string("some_string", "some\nstring\nvalue");
        object.string_unchecked("unchecked_str", "unchecked");
        object.number("some_number", Number::Float(3.5));
        object.null("some_null");

        let mut array = object.start_array("some_mixed_array");
        array
            .string("1")
            .number(Number::NegInt(-2))
            .string_unchecked("unchecked")
            .boolean(true)
            .boolean(false)
            .null();
        array.finish();

        object.finish();

        assert_eq!(
            r#"{"true_val":true,"false_val":false,"some_string":"some\nstring\nvalue","unchecked_str":"unchecked","some_number":3.5,"some_null":null,"some_mixed_array":["1",-2,"unchecked",true,false,null]}"#,
            &output
        );
    }

    #[test]
    fn object_instants() {
        let mut output = String::new();

        let mut object = JsonObjectWriter::new(&mut output);
        object.instant(
            "epoch_seconds",
            &Instant::from_f64(5.2),
            Format::EpochSeconds,
        );
        object.instant(
            "date_time",
            &Instant::from_str("2021-05-24T15:34:50.123Z", Format::DateTime).unwrap(),
            Format::DateTime,
        );
        object.instant(
            "http_date",
            &Instant::from_str("Wed, 21 Oct 2015 07:28:00 GMT", Format::HttpDate).unwrap(),
            Format::HttpDate,
        );
        object.finish();

        assert_eq!(
            r#"{"epoch_seconds":5.2,"date_time":"2021-05-24T15:34:50.123Z","http_date":"Wed, 21 Oct 2015 07:28:00 GMT"}"#,
            &output,
        )
    }

    #[test]
    fn array_instants() {
        let mut output = String::new();

        let mut array = JsonArrayWriter::new(&mut output);
        array.instant(&Instant::from_f64(5.2), Format::EpochSeconds);
        array.instant(
            &Instant::from_str("2021-05-24T15:34:50.123Z", Format::DateTime).unwrap(),
            Format::DateTime,
        );
        array.instant(
            &Instant::from_str("Wed, 21 Oct 2015 07:28:00 GMT", Format::HttpDate).unwrap(),
            Format::HttpDate,
        );
        array.finish();

        assert_eq!(
            r#"[5.2,"2021-05-24T15:34:50.123Z","Wed, 21 Oct 2015 07:28:00 GMT"]"#,
            &output,
        )
    }

    #[test]
    fn append_string_unchecked_no_escaping() {
        let mut value = String::new();
        append_string_unchecked(&mut value, "totally\ninvalid");
        assert_eq!("\"totally\ninvalid\"", &value);
    }

    fn format_test_number(number: Number) -> String {
        let mut formatted = String::new();
        append_number(&mut formatted, number);
        formatted
    }

    #[test]
    fn number_formatting() {
        let format = |n: Number| {
            let mut buffer = String::new();
            append_number(&mut buffer, n);
            buffer
        };

        assert_eq!("1", format(Number::PosInt(1)));
        assert_eq!("-1", format(Number::NegInt(-1)));
        assert_eq!("1", format(Number::NegInt(1)));
        assert_eq!("0.0", format(Number::Float(0.0)));
        assert_eq!("10000000000.0", format(Number::Float(1e10)));
        assert_eq!("-1.2", format(Number::Float(-1.2)));

        // JSON doesn't support NaN, Infinity, or -Infinity, so we're matching
        // the behavior of the serde_json crate in these cases.
        assert_eq!(
            serde_json::to_string(&f64::NAN).unwrap(),
            format_test_number(Number::Float(f64::NAN))
        );
        assert_eq!(
            serde_json::to_string(&f64::INFINITY).unwrap(),
            format_test_number(Number::Float(f64::INFINITY))
        );
        assert_eq!(
            serde_json::to_string(&f64::NEG_INFINITY).unwrap(),
            format_test_number(Number::Float(f64::NEG_INFINITY))
        );
    }

    proptest! {
        #[test]
        fn matches_serde_json_pos_int_format(value: u64) {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format_test_number(Number::PosInt(value)),
            )
        }

        #[test]
        fn matches_serde_json_neg_int_format(value: i64) {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format_test_number(Number::NegInt(value)),
            )
        }

        #[test]
        fn matches_serde_json_float_format(value: f64) {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format_test_number(Number::Float(value)),
            )
        }
    }
}
