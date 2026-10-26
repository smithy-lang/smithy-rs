/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Compiles generated schemas containing awkward names and trait strings and exercises their
 * Rust serialization. A recorder checks that serializing large blobs shares their original
 * allocation rather than copying bytes. It also verifies body-member and media-type metadata,
 * and requires a clear codegen error for unsupported recursion involving collections alone.
 */
internal class SchemaGenerationIntegrityTest {
    @Test
    fun `schema names and trait strings compile and blob serialization shares allocations`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.integrity
            use aws.protocols#restJson1
            @restJson1 service Integrity { version: "1", operations: [Read] }
            @http(method: "POST", uri: "/read")
            operation Read {
                input := { @httpPayload document: Document }
                output := { x: BlobsMap, xKey: String, xValue: Blob, l: Blobs, lMember: String, blob: Blob, media: Media }
            }
            map BlobsMap { key: String, value: Blob }
            list Blobs { member: Blob }
            @mediaType("application/x-test#suffix") string Media
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode().merge(ObjectNode.parse("""{"customizationConfig":{"protocols":{"custom#protocol":{"raw":"\"####","hash":"#value"}}}}""").expectObjectNode()),
                cargoCommand = "cargo test --quiet --lib",
            ),
            testCoverage = HttpTestType.Default,
        ) { context, crate ->
            crate.testModule {
                unitTest("blobs_reach_the_serializer_without_copying_payloads") {
                    rustTemplate(
                        """
                        use #{Schema}::serde::{SerializableStruct, ShapeSerializer, SerdeError};
                        use #{Schema}::Schema;
                        struct Addresses { expected: usize, calls: usize }
                        macro_rules! ignore {
                            (${'$'}(${'$'}method:ident(${'$'}arg:ty)),* ${'$'}(,)?) => { ${'$'}(
                                fn ${'$'}method(&mut self, _: &Schema<'_>, _: ${'$'}arg) -> Result<(), SerdeError> { Ok(()) }
                            )* };
                        }
                        impl ShapeSerializer for Addresses {
                            fn write_struct(&mut self, _: &Schema<'_>, value: &dyn SerializableStruct) -> Result<(), SerdeError> { value.serialize_members(self) }
                            fn write_list(&mut self, _: &Schema<'_>, write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>) -> Result<(), SerdeError> { write(self) }
                            fn write_map(&mut self, _: &Schema<'_>, write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>) -> Result<(), SerdeError> { write(self) }
                            fn write_blob(&mut self, _: &Schema<'_>, value: #{Types}::Blob) -> Result<(), SerdeError> {
                                assert_eq!(value.as_ref().as_ptr() as usize, self.expected);
                                assert_eq!(value.as_ref().len(), 1024 * 1024);
                                self.calls += 1;
                                Ok(())
                            }
                            fn write_null(&mut self, _: &Schema<'_>) -> Result<(), SerdeError> { Ok(()) }
                            ignore!(write_boolean(bool), write_byte(i8), write_short(i16), write_integer(i32), write_long(i64),
                                write_float(f32), write_double(f64), write_big_integer(&#{Types}::BigInteger),
                                write_big_decimal(&#{Types}::BigDecimal), write_string(&str), write_timestamp(&#{Types}::DateTime),
                                write_document(&#{Types}::Document));
                        }
                        let blob = #{Types}::Blob::new(vec![7; 1024 * 1024]);
                        let mut recorder = Addresses { expected: blob.as_ref().as_ptr() as usize, calls: 0 };
                        let output = crate::output::ReadOutput::builder()
                            .x(Some(std::collections::HashMap::from([("one".into(), blob.clone())])))
                            .x_key(Some("key".into())).x_value(Some(blob.clone()))
                            .l(Some(vec![blob.clone(), blob.clone()])).l_member(Some("member".into()))
                            .blob(Some(blob)).media(Some("media".into())).build();
                        output.serialize_members(&mut recorder).unwrap();
                        assert_eq!(recorder.calls, 5);
                        assert!(crate::input::ReadInput::SCHEMA.has_body_members());
                        assert_eq!(crate::output::ReadOutput::SCHEMA.member_schema("media").unwrap().media_type().unwrap().value(), "application/x-test#suffix");
                        """.replace("application/x-test#suffix", "application/x-test##suffix"),
                        "Schema" to RuntimeType.smithySchema(context.runtimeConfig),
                        "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                    )
                }
            }
        }
    }

    @Test
    fun `collection-only recursion fails codegen clearly`() {
        for (collections in listOf("list Cycle { member: Cycle }", "list Cycle { member: Other }\nmap Other { key: String, value: Cycle }")) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.recursive
                use aws.protocols#restJson1
                @restJson1 service Recursive { version: "1", operations: [Read] }
                @http(method: "POST", uri: "/read") operation Read { input := {}, output := { values: Cycle } }
                $collections
                """.asSmithyModel(disableValidation = true)
            val error =
                assertThrows(CodegenException::class.java) {
                    serverIntegrationTest(
                        model,
                        IntegrationTestParams(
                            additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode(),
                        ),
                        testCoverage = HttpTestType.Default,
                    )
                }
            assertTrue(error.message!!.contains("collection-only recursion"))
        }
    }
}
