/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class KotlinClientSerdeDecoratorTest {
    private val simpleModel =
        """
        namespace com.example
        use smithy.rust#serde
        use aws.protocols#awsJson1_0
        use smithy.framework#ValidationException
        @awsJson1_0
        service HelloService {
            operations: [SayHello, SayGoodBye],
            version: "1"
        }
        @optionalAuth
        operation SayHello {
            input: TestInput
            errors: [ValidationException]
        }
        @serde
        structure TestInput {
           foo: SensitiveString,
           e: TestEnum,
           nested: Nested,
           union: U
        }

        @sensitive
        string SensitiveString

        @sensitive
        enum TestEnum {
            A,
            B,
            C,
            D
        }

        @sensitive
        union U {
            nested: Nested,
            enum: TestEnum
        }

        structure Nested {
          int: Integer,
          sensitive: Timestamps,
          notSensitive: AlsoTimestamps,
          manyEnums: TestEnumList
        }

        list TestEnumList {
            member: TestEnum
        }

        map Timestamps {
            key: String
            value: SensitiveTimestamp
        }

        map AlsoTimestamps {
            key: String
            value: Timestamp
        }

        @sensitive
        timestamp SensitiveTimestamp

        operation SayGoodBye {
            input: NotSerde
        }
        structure NotSerde {}
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun generateSerializersThatWorkClient() {
        clientIntegrationTest(simpleModel, params = IntegrationTestParams(cargoCommand = "cargo test --all-features")) { ctx, crate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(ctx.moduleUseName()),
                    "serde_json" to CargoDependency("serde_json", CratesIo("1")).toDevDependency().toType(),
                )

            crate.integrationTest("test_serde") {
                unitTest("input_serialized") {
                    rustTemplate(
                        """
                        use #{crate}::types::{Nested, U};
                        use #{crate}::serde_impl::support::*;
                        use std::time::UNIX_EPOCH;
                        use aws_smithy_types::DateTime;
                        let input = #{crate}::operation::say_hello::SayHelloInput::builder()
                            .foo("foo-value")
                            .e("A".into())
                            .nested(Nested::builder()
                                .int(5)
                                .sensitive("a", DateTime::from(UNIX_EPOCH))
                                .not_sensitive("a", DateTime::from(UNIX_EPOCH))
                                .many_enums("A".into())
                                .build()
                            )
                            .union(U::Enum("B".into()))
                            .build()
                            .unwrap();
                        let mut settings = #{crate}::serde_impl::support::SerializationSettings::default();
                        let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                        assert_eq!(serialized, "{\"foo\":\"foo-value\",\"e\":\"A\",\"nested\":{\"int\":5,\"sensitive\":{\"a\":\"1970-01-01T00:00:00Z\"},\"notSensitive\":{\"a\":\"1970-01-01T00:00:00Z\"},\"manyEnums\":[\"A\"]},\"union\":{\"enum\":\"B\"}}");
                        settings.redact_sensitive_fields = true;
                        let serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                        assert_eq!(serialized, "{\"foo\":\"<redacted>\",\"e\":\"<redacted>\",\"nested\":{\"int\":5,\"sensitive\":{\"a\":\"<redacted>\"},\"notSensitive\":{\"a\":\"1970-01-01T00:00:00Z\"},\"manyEnums\":[\"<redacted>\"]},\"union\":\"<redacted>\"}");
                        """,
                        *codegenScope,
                    )
                }

                unitTest("delegated_serde") {
                    rustTemplate(
                        """
                        use #{crate}::operation::say_hello::SayHelloInput;
                        use #{crate}::serde_impl::support::*;
                        ##[derive(serde::Serialize)]
                        struct MyRecord {
                            ##[serde(serialize_with = "serialize_redacted")]
                            redact_field: SayHelloInput,
                            ##[serde(serialize_with = "serialize_unredacted")]
                            unredacted_field: SayHelloInput
                        }
                        let input = SayHelloInput::builder().foo("foo-value").build().unwrap();

                        let field = MyRecord {
                            redact_field: input.clone(),
                            unredacted_field: input
                        };
                        let serialized = #{serde_json}::to_string(&field).expect("failed to serialize");
                        assert_eq!(serialized, r##"{"redact_field":{"foo":"<redacted>"},"unredacted_field":{"foo":"foo-value"}}"##);
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
