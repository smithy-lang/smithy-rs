/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rawRust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class BigNumberPrecisionTest {
    @Test
    fun `test BigInteger and BigDecimal round trip through serializers with restJson1`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "2026-01-01",
                operations: [TestOp]
            }

            @http(uri: "/test", method: "POST")
            operation TestOp {
                input: TestInput,
                output: TestOutput
            }

            structure TestInput {
                bigInt: BigInteger,
                bigDec: BigDecimal
            }

            structure TestOutput {
                bigInt: BigInteger,
                bigDec: BigDecimal
            }
            """.asSmithyModel()

        clientIntegrationTest(model) { _, rustCrate ->
            rustCrate.unitTest("big_number_round_trip") {
                rawRust(
                    """
                    use aws_smithy_types::{BigInteger, BigDecimal};
                    use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
                    use aws_smithy_schema::serde::ShapeSerializer;
                    use aws_smithy_schema::codec::Codec;
                    use std::str::FromStr;

                    // Test values that exceed native type limits
                    let big_int_str = "99999999999999999999999999";  // > u64::MAX
                    let big_dec_precision_str = "3.141592653589793238462643383279502884197";  // > f64 precision (15-17 digits)
                    let big_dec_magnitude_str = "1.8e308";  // > f64::MAX - tokenizer uses NaN for validation

                    // Test 1: High precision BigDecimal - serialize via codec
                    let input = crate::operation::test_op::TestOpInput::builder()
                        .big_int(BigInteger::from_str(big_int_str).unwrap())
                        .big_dec(BigDecimal::from_str(big_dec_precision_str).unwrap())
                        .build()
                        .unwrap();

                    let codec = JsonCodec::new(JsonCodecSettings::builder().use_json_name(true).build());
                    let mut ser = codec.create_serializer();
                    ser.write_struct(&crate::operation::test_op::TestOpInput::SCHEMA, &input).unwrap();
                    let json_body = ser.finish();
                    let serialized = String::from_utf8(json_body).unwrap();

                    assert!(serialized.contains(big_int_str));
                    assert!(serialized.contains(big_dec_precision_str));

                    // Test 2: Large magnitude BigDecimal - deserialize via codec
                    let mut json_response = String::from(r#"{"bigInt":"#);
                    json_response.push_str(big_int_str);
                    json_response.push_str(r#","bigDec":"#);
                    json_response.push_str(big_dec_magnitude_str);
                    json_response.push('}');

                    let mut deser = codec.create_deserializer(json_response.as_bytes());
                    let output = crate::operation::test_op::TestOpOutput::deserialize(&mut deser).unwrap();

                    assert_eq!(output.big_int.unwrap().as_ref(), big_int_str);
                    assert_eq!(output.big_dec.unwrap().as_ref(), big_dec_magnitude_str);
                    """,
                )
            }
        }
    }

    @Test
    fun `test BigInteger and BigDecimal round trip through serializers with restXml`() {
        val model =
            """
            namespace test
            use aws.protocols#restXml

            @restXml
            service TestService {
                version: "2026-01-01",
                operations: [TestOp]
            }

            @http(uri: "/test", method: "POST")
            operation TestOp {
                input: TestInput,
                output: TestOutput
            }

            structure TestInput {
                bigInt: BigInteger,
                bigDec: BigDecimal
            }

            structure TestOutput {
                bigInt: BigInteger,
                bigDec: BigDecimal
            }
            """.asSmithyModel()

        clientIntegrationTest(model) { _, rustCrate ->
            rustCrate.unitTest("big_number_round_trip_xml") {
                rawRust(
                    """
                    use aws_smithy_types::{BigInteger, BigDecimal};
                    use std::str::FromStr;

                    // Test values that exceed native type limits
                    let big_int_str = "99999999999999999999999999";  // > u64::MAX
                    let big_dec_precision_str = "3.141592653589793238462643383279502884197";  // > f64 precision (15-17 digits)
                    let big_dec_magnitude_str = "1.8e308";  // > f64::MAX (~1.7976931348623157e308)

                    // Test 1: High precision BigDecimal
                    let input = crate::operation::test_op::TestOpInput::builder()
                        .big_int(BigInteger::from_str(big_int_str).unwrap())
                        .big_dec(BigDecimal::from_str(big_dec_precision_str).unwrap())
                        .build()
                        .unwrap();

                    let xml_body = crate::protocol_serde::shape_test_op::ser_test_op_op_input(&input).unwrap();
                    let serialized = String::from_utf8(xml_body.bytes().unwrap().to_vec()).unwrap();

                    assert!(serialized.contains(big_int_str));
                    assert!(serialized.contains(big_dec_precision_str));

                    // Test 2: Large magnitude BigDecimal - construct XML manually
                    let mut xml_response = String::from(r#"<TestOutput><bigInt>"#);
                    xml_response.push_str(big_int_str);
                    xml_response.push_str(r#"</bigInt><bigDec>"#);
                    xml_response.push_str(big_dec_magnitude_str);
                    xml_response.push_str(r#"</bigDec></TestOutput>"#);

                    let headers = ::aws_smithy_runtime_api::http::Headers::new();
                    let output = crate::protocol_serde::shape_test_op::de_test_op_http_response(
                        200,
                        &headers,
                        xml_response.as_bytes()
                    ).unwrap();

                    assert_eq!(output.big_int.unwrap().as_ref(), big_int_str);
                    assert_eq!(output.big_dec.unwrap().as_ref(), big_dec_magnitude_str);
                    """,
                )
            }
        }
    }

    @Test
    fun `test BigInteger and BigDecimal round trip through serializers with awsJson1_1`() {
        val model =
            """
            namespace test
            use aws.protocols#awsJson1_1

            @awsJson1_1
            service TestService {
                version: "2023-01-01",
                operations: [TestOp]
            }

            operation TestOp {
                input: TestInput,
                output: TestOutput
            }

            structure TestInput {
                bigInt: BigInteger,
                bigDec: BigDecimal
            }

            structure TestOutput {
                bigInt: BigInteger,
                bigDec: BigDecimal
            }
            """.asSmithyModel()

        clientIntegrationTest(model) { _, rustCrate ->
            rustCrate.unitTest("big_number_round_trip_aws_json") {
                rawRust(
                    """
                    use aws_smithy_types::{BigInteger, BigDecimal};
                    use aws_smithy_json::codec::JsonCodec;
                    use aws_smithy_schema::serde::ShapeSerializer;
                    use aws_smithy_schema::codec::Codec;
                    use std::str::FromStr;

                    // Test values that exceed native type limits
                    let big_int_str = "99999999999999999999999999";  // > u64::MAX
                    let big_dec_precision_str = "3.141592653589793238462643383279502884197";  // > f64 precision
                    let big_dec_magnitude_str = "1.8e308";  // > f64::MAX

                    // Test 1: High precision BigDecimal - serialize via codec
                    let input = crate::operation::test_op::TestOpInput::builder()
                        .big_int(BigInteger::from_str(big_int_str).unwrap())
                        .big_dec(BigDecimal::from_str(big_dec_precision_str).unwrap())
                        .build()
                        .unwrap();

                    let codec = JsonCodec::default();
                    let mut ser = codec.create_serializer();
                    ser.write_struct(&crate::operation::test_op::TestOpInput::SCHEMA, &input).unwrap();
                    let json_body = ser.finish();
                    let serialized = String::from_utf8(json_body).unwrap();

                    assert!(serialized.contains(big_int_str));
                    assert!(serialized.contains(big_dec_precision_str));

                    // Test 2: Large magnitude BigDecimal - deserialize via codec
                    let mut json_response = String::from(r#"{"bigInt":"#);
                    json_response.push_str(big_int_str);
                    json_response.push_str(r#","bigDec":"#);
                    json_response.push_str(big_dec_magnitude_str);
                    json_response.push('}');

                    let mut deser = codec.create_deserializer(json_response.as_bytes());
                    let output = crate::operation::test_op::TestOpOutput::deserialize(&mut deser).unwrap();

                    assert_eq!(output.big_int.unwrap().as_ref(), big_int_str);
                    assert_eq!(output.big_dec.unwrap().as_ref(), big_dec_magnitude_str);
                    """,
                )
            }
        }
    }
}
