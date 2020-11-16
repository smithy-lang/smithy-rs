/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider

class UnionGeneratorTest {
    @Test
    fun `generate basic unions`() {
        val member1 = MemberShape.builder()
            .id("com.test#MyUnion\$stringConfig")
            .target("smithy.api#String").build()
        val member2 = MemberShape.builder().id("com.test#MyUnion\$intConfig")
            .target("smithy.api#PrimitiveInteger").addTrait(
                DocumentationTrait("This *is* documentation about the member.")
            ).build()

        val union = UnionShape.builder()
            .id("com.test#MyUnion")
            .addMember(member1)
            .addMember(member2)
            .build()

        val model = Model.assembler()
            .addShapes(union, member1, member2)
            .assemble()
            .unwrap()
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val generator = UnionGenerator(model, provider, writer, union)
        generator.render()
        writer.compileAndTest(
            """
        let var_a = MyUnion::StringConfig("abc".to_string());
        let var_b = MyUnion::IntConfig(10);
        assert_ne!(var_a, var_b);
        assert_eq!(var_a, var_a);
        """
        )
    }
}
