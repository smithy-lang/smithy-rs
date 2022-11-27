/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import io.kotest.matchers.string.shouldContainInOrder
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
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

                secretKey: SecretKey
            }

            structure StructWithDoc {
                doc: Document
            }
            """.asSmithyModel()
        val struct = model.lookup<StructureShape>("com.test#MyStruct")
        val structWithDoc = model.lookup<StructureShape>("com.test#StructWithDoc")
        val inner = model.lookup<StructureShape>("com.test#Inner")
        val credentials = model.lookup<StructureShape>("com.test#Credentials")
        val error = model.lookup<StructureShape>("com.test#MyError")
    }

    @Test
    fun `generate basic structures`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.useShapeWriter(inner) {
            StructureGenerator(model, provider, this, inner).render()
            StructureGenerator(model, provider, this, struct).render()
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
        val project = TestWorkspace.testProject()
        val provider = testSymbolProvider(model)

        project.lib { rust("##![allow(deprecated)]") }
        project.withModule(ModelsModule) {
            val innerGenerator = StructureGenerator(model, provider, this, inner)
            innerGenerator.render()
        }
        project.withModule(RustModule.public("structs")) {
            val generator = StructureGenerator(model, provider, this, struct)
            generator.render()
        }
        // By putting the test in another module, it can't access the struct
        // fields if they are private
        project.unitTest {
            raw("#[test]")
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
    fun `generate error structures`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("error")
        val generator = StructureGenerator(model, provider, writer, error)
        generator.render()
        writer.compileAndTest(
            """
            let err = MyError { message: None };
            assert_eq!(err.retryable_error_kind(), aws_smithy_types::retry::ErrorKind::ServerError);
            """,
        )
    }

    @Test
    fun `generate a custom debug implementation when the sensitive trait is present`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("lib")
        val generator = StructureGenerator(model, provider, writer, credentials)
        generator.render()
        writer.unitTest(
            "sensitive_fields_redacted",
            """
            let creds = Credentials {
                username: Some("not_redacted".to_owned()),
                password: Some("don't leak me".to_owned()),
                secret_key: Some("don't leak me".to_owned())
            };
            assert_eq!(format!("{:?}", creds), "Credentials { username: Some(\"not_redacted\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\" }");
            """,
        )
        writer.compileAndTest()
    }

    @Test
    fun `attach docs to everything`() {
        val model = """
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
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.lib {
            Attribute.Custom("deny(missing_docs)").render(this)
        }
        project.withModule(ModelsModule) {
            StructureGenerator(model, provider, this, model.lookup("com.test#Inner")).render()
            StructureGenerator(model, provider, this, model.lookup("com.test#MyStruct")).render()
        }

        project.compileAndTest()
    }

    @Test
    fun `documents are optional in structs`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("lib")
        StructureGenerator(model, provider, writer, structWithDoc).render()

        writer.compileAndTest(
            """
            let _struct = StructWithDoc {
                // This will only compile if the document is optional
                doc: None
            };
            """,
        )
    }

    @Test
    fun `deprecated trait with message and since`() {
        val model = """
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
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.lib { rust("##![allow(deprecated)]") }
        project.withModule(ModelsModule) {
            StructureGenerator(model, provider, this, model.lookup("test#Foo")).render()
            StructureGenerator(model, provider, this, model.lookup("test#Bar")).render()
            StructureGenerator(model, provider, this, model.lookup("test#Baz")).render()
            StructureGenerator(model, provider, this, model.lookup("test#Qux")).render()
        }

        // turn on clippy to check the semver-compliant version of `since`.
        project.compileAndTest(runClippy = true)
    }

    @Test
    fun `nested deprecated trait`() {
        val model = """
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
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.lib { rust("##![allow(deprecated)]") }
        project.withModule(ModelsModule) {
            StructureGenerator(model, provider, this, model.lookup("test#Nested")).render()
            StructureGenerator(model, provider, this, model.lookup("test#Foo")).render()
            StructureGenerator(model, provider, this, model.lookup("test#Bar")).render()
        }

        project.compileAndTest()
    }

    @Test
    fun `it generates accessor methods`() {
        val testModel =
            RecursiveShapeBoxer.transform(
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
        val provider = testSymbolProvider(testModel)
        val project = TestWorkspace.testProject(provider)

        project.useShapeWriter(inner) {
            StructureGenerator(testModel, provider, this, testModel.lookup("test#One")).render()
            StructureGenerator(testModel, provider, this, testModel.lookup("test#Two")).render()

            rustBlock("fn compile_test_one(one: &crate::model::One)") {
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
                    let _: Option<&crate::model::Two> = one.two();
                    let _: Option<i32> = one.build_value();
                    let _: Option<i32> = one.builder_value();
                    let _: Option<i32> = one.default_value();
                    """,
                )
            }
            rustBlock("fn compile_test_two(two: &crate::model::Two)") {
                rust(
                    """
                    let _: Option<&crate::model::One> = two.one();
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `non-streaming fields are doc-hidden`() {
        val model = """
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

        val provider = testSymbolProvider(model)
        RustWriter.forModule("test").let {
            StructureGenerator(model, provider, it, struct).render()
            assertEquals(6, it.toString().split("#[doc(hidden)]").size, "there should be 5 doc-hiddens")
        }
    }

    @Test
    fun `streaming fields are NOT doc-hidden`() {
        val model = """
            namespace com.test
            @streaming blob SomeStreamingThing
            structure MyStruct { foo: SomeStreamingThing }
        """.asSmithyModel()
        val struct = model.lookup<StructureShape>("com.test#MyStruct")

        val provider = testSymbolProvider(model)
        RustWriter.forModule("test").let { writer ->
            StructureGenerator(model, provider, writer, struct).render()
            writer.toString().shouldNotContain("#[doc(hidden)]")
        }
    }
}
