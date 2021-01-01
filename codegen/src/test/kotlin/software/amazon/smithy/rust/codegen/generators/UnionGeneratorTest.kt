/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider

class UnionGeneratorTest {
    @Test
    fun `generate basic unions`() {
        val model = """
        namespace test
        union MyUnion {
            stringConfig: String,
            @documentation("This *is* documentation about the member")
            intConfig: PrimitiveInteger
        }
        """.asSmithyModel()
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val generator = UnionGenerator(model, provider, writer, model.lookup("test#MyUnion"))
        generator.render()
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
}
