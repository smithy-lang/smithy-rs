/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.StringTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaTraitFilter
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class ServerSchemaGeneratorTest {
    @Test
    fun `unknown string trait values preserve template characters`() {
        val baseModel =
            """
            namespace test
            structure Tagged {}
            """.asSmithyModel()
        val traitId = ShapeId.from("test#custom")
        for (value in listOf("plain", "a#b", "#{missing}", "a##b")) {
            // Model-defined custom traits are DynamicTrait instances. Use a JVM
            // StringTrait implementation to exercise the string fallback.
            val trait = object : StringTrait(traitId, value, SourceLocation.NONE) {}
            val shape = baseModel.lookup<StructureShape>("test#Tagged").toBuilder().addTrait(trait).build()
            val model = baseModel.toBuilder().addShape(shape).build()
            val writer = RustWriter.forModule("model")

            ServerSchemaGenerator(
                serverTestCodegenContext(model),
                writer,
                shape,
                traitFilter = SchemaTraitFilter(model, setOf(traitId)),
            ).renderSerializeOnly()

            writer.toString() shouldContain """::from_parts("test#custom", "test", "custom"), "$value")));"""
        }
    }
}
