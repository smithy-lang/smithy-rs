/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordConfig
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.lookup

class UnionGeneratorTest {
    @Test
    fun `generate basic unions`() {
        val writer =
            generateUnion(
                """
                union MyUnion {
                    stringConfig: String,
                    @documentation("This *is* documentation about the member")
                    intConfig: PrimitiveInteger
                }
                """,
            )

        writer.compileAndTest(
            """
            let var_a = MyUnion::StringConfig("abc".to_string());
            let var_b = MyUnion::IntConfig(10);
            assert_ne!(var_a, var_b);
            assert_eq!(var_a, var_a);
            """,
        )
        writer.toString() shouldContain "#[non_exhaustive]"
    }

    @Test
    fun `generate basic union with member names Unknown`() {
        val writer =
            generateUnion(
                """
                union MyUnion {
                    unknown: String
                }
                """,
            )

        writer.compileAndTest(
            """
            let var_a = MyUnion::UnknownValue("abc".to_string());
            let var_b = MyUnion::Unknown;
            assert_ne!(var_a, var_b);
            assert_eq!(var_a, var_a);
            """,
        )
        writer.toString() shouldContain "#[non_exhaustive]"
    }

    @Test
    fun `generate conversion helper methods`() {
        val writer =
            generateUnion(
                """
                union MyUnion {
                    stringValue: String,
                    intValue: PrimitiveInteger
                }
                """,
            )

        writer.compileAndTest(
            """
            let foo = MyUnion::StringValue("foo".to_string());
            let bar = MyUnion::IntValue(10);
            assert_eq!(foo.is_string_value(), true);
            assert_eq!(foo.is_int_value(), false);
            assert_eq!(foo.as_string_value(), Ok(&"foo".to_string()));
            assert_eq!(foo.as_int_value(), Err(&foo));
            assert_eq!(bar.is_string_value(), false);
            assert_eq!(bar.is_int_value(), true);
            assert_eq!(bar.as_string_value(), Err(&bar));
            assert_eq!(bar.as_int_value(), Ok(&10));
            """,
        )
    }

    @Test
    fun `documents are not optional in unions`() {
        val writer = generateUnion("union MyUnion { doc: Document, other: String }")
        writer.compileAndTest(
            """
            // If the document isn't optional, this will compile
            MyUnion::Doc(aws_smithy_types::Document::Null);
            """,
        )
    }

    @Test
    fun `render a union without an unknown variant`() {
        val writer = generateUnion("union MyUnion { a: String, b: String }", unknownVariant = false)
        writer.compileAndTest()
    }

    @Test
    fun `render an unknown variant`() {
        val writer = generateUnion("union MyUnion { a: String, b: String }", unknownVariant = true)
        writer.compileAndTest(
            """
            let union = MyUnion::Unknown;
            assert!(union.is_unknown());

            """,
        )
    }

    @Test
    fun `generate deprecated unions`() {
        val model =
            """namespace test
            union Nested {
                foo: Foo,
                @deprecated
                foo2: Foo,
            }
            @deprecated
            union Foo {
                bar: Bar,
            }

            @deprecated
            union Bar { x: Integer }
            """.asSmithyModel()
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.lib { rust("##![allow(deprecated)]") }
        project.moduleFor(model.lookup("test#Nested")) {
            UnionGenerator(model, provider, this, model.lookup("test#Nested")).render()
            UnionGenerator(model, provider, this, model.lookup("test#Foo")).render()
            UnionGenerator(model, provider, this, model.lookup("test#Bar")).render()
        }

        project.compileAndTest()
    }

    @Test
    fun `impl debug for non-sensitive union should implement the derived debug trait`() {
        val writer =
            generateUnion(
                """
                union MyUnion {
                    foo: PrimitiveInteger
                    bar: String,
                }
                """,
            )

        writer.compileAndTest(
            """
            assert_eq!(format!("{:?}", MyUnion::Foo(3)), "Foo(3)");
            assert_eq!(format!("{:?}", MyUnion::Bar("bar".to_owned())), "Bar(\"bar\")");
            """,
        )
    }

    @Test
    fun `impl debug for sensitive union should redact text`() {
        val writer =
            generateUnion(
                """
                @sensitive
                union MyUnion {
                    foo: PrimitiveInteger,
                    bar: String,
                }
                """,
            )

        writer.compileAndTest(
            """
            assert_eq!(format!("{:?}", MyUnion::Foo(3)), $REDACTION);
            assert_eq!(format!("{:?}", MyUnion::Bar("bar".to_owned())), $REDACTION);
            """,
        )
    }

    @Test
    fun `impl debug for union should redact text for sensitive member target`() {
        val writer =
            generateUnion(
                """
                @sensitive
                string Bar

                union MyUnion {
                    foo: PrimitiveInteger,
                    bar: Bar,
                }
                """,
            )

        writer.compileAndTest(
            """
            assert_eq!(format!("{:?}", MyUnion::Foo(3)), "Foo(3)");
            assert_eq!(format!("{:?}", MyUnion::Bar("bar".to_owned())), $REDACTION);
            """,
        )
    }

    @Test
    fun `impl debug for union with unit target should redact text for sensitive member target`() {
        val writer =
            generateUnion(
                """
                @sensitive
                string Bar

                union MyUnion {
                    foo: Unit,
                    bar: Bar,
                }
                """,
            )

        writer.compileAndTest(
            """
            assert_eq!(format!("{:?}", MyUnion::Foo), "Foo");
            assert_eq!(format!("{:?}", MyUnion::Bar("bar".to_owned())), $REDACTION);
            """,
        )
    }

    @Test
    fun `unit types should not appear in generated enum`() {
        val writer = generateUnion("union MyUnion { a: Unit, b: String }", unknownVariant = true)
        writer.compileAndTest(
            """
            let a = MyUnion::A;
            assert_eq!(Ok(()), a.as_a());
            """,
        )
    }

    private fun generateUnion(
        modelSmithy: String,
        unionName: String = "MyUnion",
        unknownVariant: Boolean = true,
    ): RustWriter {
        val model = "namespace test\n$modelSmithy".asSmithyModel()
        // Reserved words to test generation of renamed members
        val reservedWords =
            RustReservedWordConfig(
                structureMemberMap =
                    StructureGenerator.structureMemberNameMap,
                unionMemberMap =
                    mapOf(
                        // Unions contain an `Unknown` variant. This exists to support parsing data returned from the server
                        // that represent union variants that have been added since this SDK was generated.
                        UnionGenerator.UNKNOWN_VARIANT_NAME to "${UnionGenerator.UNKNOWN_VARIANT_NAME}Value",
                        "${UnionGenerator.UNKNOWN_VARIANT_NAME}Value" to "${UnionGenerator.UNKNOWN_VARIANT_NAME}Value_",
                    ),
                enumMemberMap =
                    mapOf(),
            )
        val provider: RustSymbolProvider = testSymbolProvider(model)
        val reservedWordsProvider = RustReservedWordSymbolProvider(provider, reservedWords)
        val writer = RustWriter.forModule("model")
        UnionGenerator(
            model,
            reservedWordsProvider,
            writer,
            model.lookup("test#$unionName"),
            renderUnknownVariant = unknownVariant,
        ).render()
        return writer
    }
}
