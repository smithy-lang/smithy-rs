/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import io.kotest.matchers.string.shouldContainInOrder
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordConfig
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class StructureGeneratorTest {
    companion object {
        val model =
            """
            namespace com.test
            @documentation("this documents the shape")
            structure MyStruct {
               foo: String,
               @documentation("This *is* documentation about the member.")
               bar: PrimitiveInteger,
               // Intentionally deprecated.
               @deprecated
               baz: Integer,
               ts: Timestamp,
               inner: Inner,
               byteValue: Byte,
            }

            // Intentionally empty
            structure Inner {
            }

            @error("server")
            @retryable
            structure MyError {
                message: String
            }

            @sensitive
            string SecretKey

            @sensitive
            string Password

            structure Credentials {
                username: String,
                password: Password,
                secretKey: SecretKey,
                secretValueMap: MapThatContainsSecretValues,
                secretKeyMap: MapThatContainsSecretKeys,
                secretList: ListThatContainsSecrets
            }

            structure StructWithDoc {
                doc: Document
            }

            @sensitive
            structure SecretStructure {
                secretField: String
            }

            structure StructWithInnerSecretStructure {
                public: String,
                private: SecretStructure,
            }

            map MapThatContainsSecretKeys {
                key: SecretKey
                value: String
            }

            map MapThatContainsSecretValues {
                key: String
                value: SecretKey
            }

            list ListThatContainsSecrets {
                member: Password
            }
            """.asSmithyModel()
        val struct = model.lookup<StructureShape>("com.test#MyStruct")
        val structWithDoc = model.lookup<StructureShape>("com.test#StructWithDoc")
        val inner = model.lookup<StructureShape>("com.test#Inner")
        val credentials = model.lookup<StructureShape>("com.test#Credentials")
        val secretStructure = model.lookup<StructureShape>("com.test#SecretStructure")
        val structWithInnerSecretStructure = model.lookup<StructureShape>("com.test#StructWithInnerSecretStructure")
        val error = model.lookup<StructureShape>("com.test#MyError")

        val rustReservedWordConfig: RustReservedWordConfig =
            RustReservedWordConfig(
                structureMemberMap = StructureGenerator.structureMemberNameMap,
                enumMemberMap = emptyMap(),
                unionMemberMap = emptyMap(),
            )
    }

    private fun structureGenerator(
        model: Model,
        provider: RustSymbolProvider,
        writer: RustWriter,
        shape: StructureShape,
    ) = StructureGenerator(model, provider, writer, shape, emptyList(), StructSettings(flattenVecAccessors = true))

    @Test
    fun `generate basic structures`() {
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        val project = TestWorkspace.testProject(provider)
        project.useShapeWriter(inner) {
            structureGenerator(model, provider, this, inner).render()
            structureGenerator(model, provider, this, struct).render()
            unitTest(
                "struct_fields_optional",
                """
                let s: Option<MyStruct> = None;
                s.map(|i|println!("{:?}, {:?}", i.ts, i.byte_value));
                """,
            )
            toString().shouldContainInOrder(
                "this documents the shape", "#[non_exhaustive]", "pub", "struct MyStruct",
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `generate structures with public fields`() {
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        val project = TestWorkspace.testProject(provider)

        project.lib { Attribute.AllowDeprecated.render(this) }
        project.moduleFor(inner) {
            val innerGenerator = structureGenerator(model, provider, this, inner)
            innerGenerator.render()
        }
        project.withModule(RustModule.public("structs")) {
            val generator = structureGenerator(model, provider, this, struct)
            generator.render()
        }
        // By putting the test in another module, it can't access the struct
        // fields if they are private
        project.unitTest {
            Attribute.Test.render(this)
            rustBlock("fn test_public_fields()") {
                write(
                    """
                    let s: Option<crate::structs::MyStruct> = None;
                    s.map(|i|println!("{:?}, {:?}", i.ts, i.byte_value));
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `generate a custom debug implementation when the sensitive trait is applied to some members`() {
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        TestWorkspace.testProject().unitTest {
            structureGenerator(model, provider, this, credentials).render()

            rust(
                """
                use std::collections::HashMap;

                let mut secret_map = HashMap::new();
                secret_map.insert("FirstSecret".to_string(), "don't leak me".to_string());
                secret_map.insert("SecondSecret".to_string(), "don't leak me".to_string());

                let secret_list = vec!["don't leak me".to_string()];

                let creds = Credentials {
                    username: Some("not_redacted".to_owned()),
                    password: Some("don't leak me".to_owned()),
                    secret_key: Some("don't leak me".to_owned()),
                    secret_key_map: Some(secret_map.clone()),
                    secret_value_map: Some(secret_map),
                    secret_list: Some(secret_list),
                };

                assert_eq!(format!("{:?}", creds),
                "Credentials { username: Some(\"not_redacted\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\", secret_value_map: \"*** Sensitive Data Redacted ***\", secret_key_map: \"*** Sensitive Data Redacted ***\", secret_list: \"*** Sensitive Data Redacted ***\" }");
                """,
            )
        }.compileAndTest()
    }

    @Test
    fun `generate a custom debug implementation when the sensitive trait is applied to the struct`() {
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        TestWorkspace.testProject().unitTest {
            structureGenerator(model, provider, this, secretStructure).render()

            rust(
                """
                let secret_structure = SecretStructure {
                    secret_field: Some("secret".to_owned()),
                };
                assert_eq!(format!("{:?}", secret_structure), "SecretStructure { secret_field: \"*** Sensitive Data Redacted ***\" }");
                """,
            )
        }.compileAndTest()
    }

    @Test
    fun `generate a custom debug implementation when the sensitive trait is applied to an inner struct`() {
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        val project = TestWorkspace.testProject(provider)
        project.useShapeWriter(inner) {
            val secretGenerator = structureGenerator(model, provider, this, secretStructure)
            val generator = structureGenerator(model, provider, this, structWithInnerSecretStructure)
            secretGenerator.render()
            generator.render()
            unitTest(
                "sensitive_inner_structure_redacted",
                """
                let secret_structure = SecretStructure {
                    secret_field: Some("secret".to_owned()),
                };
                let struct_with_inner_secret_structure = StructWithInnerSecretStructure {
                    public: Some("Public".to_owned()),
                    private: Some(secret_structure),
                };
                assert_eq!(format!("{:?}", struct_with_inner_secret_structure), "StructWithInnerSecretStructure { public: Some(\"Public\"), private: \"*** Sensitive Data Redacted ***\" }");
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `attach docs to everything`() {
        val model =
            """
            namespace com.test
            @documentation("inner doc")
            structure Inner { }

            @documentation("shape doc")
            structure MyStruct {
               @documentation("member doc")
               member: String,

               @documentation("specific docs")
               nested: Inner,

               nested2: Inner
        }""".asSmithyModel()
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        val project = TestWorkspace.testProject(provider)
        project.lib {
            Attribute.DenyMissingDocs.render(this)
        }
        project.moduleFor(model.lookup("com.test#Inner")) {
            structureGenerator(model, provider, this, model.lookup("com.test#Inner")).render()
            structureGenerator(model, provider, this, model.lookup("com.test#MyStruct")).render()
        }

        project.compileAndTest()
    }

    @Test
    fun `documents are optional in structs`() {
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        TestWorkspace.testProject().unitTest {
            structureGenerator(model, provider, this, structWithDoc).render()
            rust(
                """
                let _struct = StructWithDoc {
                    // This will only compile if the document is optional
                    doc: None
                };
                """.trimIndent(),
            )
        }.compileAndTest()
    }

    @Test
    fun `deprecated trait with message and since`() {
        val model =
            """
            namespace test

            @deprecated
            structure Foo {}

            @deprecated(message: "Fly, you fools!")
            structure Bar {}

            @deprecated(since: "1.2.3")
            structure Baz {}

            @deprecated(message: "Fly, you fools!", since: "1.2.3")
            structure Qux {}
            """.asSmithyModel()
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        val project = TestWorkspace.testProject(provider)
        project.lib { rust("##![allow(deprecated)]") }
        project.moduleFor(model.lookup("test#Foo")) {
            structureGenerator(model, provider, this, model.lookup("test#Foo")).render()
            structureGenerator(model, provider, this, model.lookup("test#Bar")).render()
            structureGenerator(model, provider, this, model.lookup("test#Baz")).render()
            structureGenerator(model, provider, this, model.lookup("test#Qux")).render()
        }

        // turn on clippy to check the semver-compliant version of `since`.
        project.compileAndTest(runClippy = true)
    }

    @Test
    fun `nested deprecated trait`() {
        val model =
            """
            namespace test

            structure Nested {
                foo: Foo,
                @deprecated
                foo2: Foo,
            }

            @deprecated
            structure Foo {
                bar: Bar,
            }

            @deprecated
            structure Bar {}
            """.asSmithyModel()
        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        val project = TestWorkspace.testProject(provider)
        project.lib { rust("##![allow(deprecated)]") }
        project.moduleFor(model.lookup("test#Nested")) {
            structureGenerator(model, provider, this, model.lookup("test#Nested")).render()
            structureGenerator(model, provider, this, model.lookup("test#Foo")).render()
            structureGenerator(model, provider, this, model.lookup("test#Bar")).render()
        }

        project.compileAndTest()
    }

    @Test
    fun `it generates accessor methods`() {
        val testModel =
            RecursiveShapeBoxer().transform(
                """
                namespace test

                structure One {
                    fieldString: String,
                    fieldBlob: Blob,
                    fieldTimestamp: Timestamp,
                    fieldDocument: Document,
                    fieldBoolean: Boolean,
                    fieldPrimitiveBoolean: PrimitiveBoolean,
                    fieldByte: Byte,
                    fieldPrimitiveByte: PrimitiveByte,
                    fieldShort: Short,
                    fieldPrimitiveShort: PrimitiveShort,
                    fieldInteger: Integer,
                    fieldPrimitiveInteger: PrimitiveInteger,
                    fieldLong: Long,
                    fieldPrimitiveLong: PrimitiveLong,
                    fieldFloat: Float,
                    fieldPrimitiveFloat: PrimitiveFloat,
                    fieldDouble: Double,
                    fieldPrimitiveDouble: PrimitiveDouble,
                    two: Two,
                    build: Integer,
                    builder: Integer,
                    default: Integer,
                }

                structure Two {
                    one: One,
                }
                """.asSmithyModel(),
            )
        val provider = testSymbolProvider(testModel, rustReservedWordConfig = rustReservedWordConfig)
        val project = TestWorkspace.testProject(provider)

        project.useShapeWriter(inner) {
            structureGenerator(testModel, provider, this, testModel.lookup("test#One")).render()
            structureGenerator(testModel, provider, this, testModel.lookup("test#Two")).render()

            rustBlock("fn compile_test_one(one: &crate::test_model::One)") {
                rust(
                    """
                    let _: Option<&str> = one.field_string();
                    let _: Option<&aws_smithy_types::Blob> = one.field_blob();
                    let _: Option<&aws_smithy_types::DateTime> = one.field_timestamp();
                    let _: Option<&aws_smithy_types::Document> = one.field_document();
                    let _: Option<bool> = one.field_boolean();
                    let _: bool = one.field_primitive_boolean();
                    let _: Option<i8> = one.field_byte();
                    let _: i8 = one.field_primitive_byte();
                    let _: Option<i16> = one.field_short();
                    let _: i16 = one.field_primitive_short();
                    let _: Option<i32> = one.field_integer();
                    let _: i32 = one.field_primitive_integer();
                    let _: Option<i64> = one.field_long();
                    let _: i64 = one.field_primitive_long();
                    let _: Option<f32> = one.field_float();
                    let _: f32 = one.field_primitive_float();
                    let _: Option<f64> = one.field_double();
                    let _: f64 = one.field_primitive_double();
                    let _: Option<&crate::test_model::Two> = one.two();
                    let _: Option<i32> = one.build_value();
                    let _: Option<i32> = one.builder_value();
                    let _: Option<i32> = one.default_value();
                    """,
                )
            }
            rustBlock("fn compile_test_two(two: &crate::test_model::Two)") {
                rust(
                    """
                    let _: Option<&crate::test_model::One> = two.one();
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `fields are NOT doc-hidden`() {
        val model =
            """
            namespace com.test
            structure MyStruct {
               foo: String,
               bar: PrimitiveInteger,
               baz: Integer,
               ts: Timestamp,
               byteValue: Byte,
            }
            """.asSmithyModel()
        val struct = model.lookup<StructureShape>("com.test#MyStruct")

        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        RustWriter.forModule("test").let { writer ->
            structureGenerator(model, provider, writer, struct).render()
            writer.toString().shouldNotContain("#[doc(hidden)]")
        }
    }

    @Test
    fun `streaming fields are NOT doc-hidden`() {
        val model =
            """
            namespace com.test
            @streaming blob SomeStreamingThing
            structure MyStruct { foo: SomeStreamingThing }
            """.asSmithyModel()
        val struct = model.lookup<StructureShape>("com.test#MyStruct")

        val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)
        RustWriter.forModule("test").let { writer ->
            structureGenerator(model, provider, writer, struct).render()
            writer.toString().shouldNotContain("#[doc(hidden)]")
        }
    }
}
