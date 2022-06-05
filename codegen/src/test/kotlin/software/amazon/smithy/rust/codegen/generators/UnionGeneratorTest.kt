/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.lookup

class UnionGeneratorTest {
    @Test
    fun `generate basic unions`() {
        val writer = generateUnion(
            """
            union MyUnion {
                stringConfig: String,
                @documentation("This *is* documentation about the member")
                intConfig: PrimitiveInteger
            }
            """
        )

        writer.compileAndTest(
            """
            let var_a = MyUnion::StringConfig("abc".to_string());
            let var_b = MyUnion::IntConfig(10);
            assert_ne!(var_a, var_b);
            assert_eq!(var_a, var_a);
            """
        )
        writer.toString() shouldContain "#[non_exhaustive]"
    }

    @Test
    fun `generate conversion helper methods`() {
        val writer = generateUnion(
            """
            union MyUnion {
                stringValue: String,
                intValue: PrimitiveInteger
            }
            """
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
            """
        )
    }

    @Test
    fun `documents are not optional in unions`() {
        val writer = generateUnion("union MyUnion { doc: Document, other: String }")
        writer.compileAndTest(
            """
            // If the document isn't optional, this will compile
            MyUnion::Doc(aws_smithy_types::Document::Null);
            """
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

            """
        )
    }

    private fun generateUnion(modelSmithy: String, unionName: String = "MyUnion", unknownVariant: Boolean = true): RustWriter {
        val model = "namespace test\n$modelSmithy".asSmithyModel()
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        UnionGenerator(model, provider, writer, model.lookup("test#$unionName"), renderUnknownVariant = unknownVariant).render()
        return writer
    }
}
