/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import java.io.File

internal class MultiProtocolServerModelTest {
    private val models = File("../codegen-server-test/integration-tests/multi-protocol/models")
    private val serviceId = ShapeId.from("com.example.multiprotocol#MultiProtocolService")

    @Test
    fun `server extensions retain all server protocol cases`() {
        val model =
            Model.assembler().discoverModels()
                .addImport(models.resolve("multi-protocol-common.smithy").toPath())
                .addImport(models.resolve("multi-protocol-server.smithy").toPath())
                .assemble().unwrap()
        val service = model.expectShape(serviceId, ServiceShape::class.java)
        check(service.operations.size == 11)
        var cases = 0
        model.shapes().forEach { shape ->
            if (shape.id.namespace != serviceId.namespace) return@forEach
            shape.getTrait(HttpRequestTestsTrait::class.java).orElse(null)?.let {
                check(it.getTestCasesFor(AppliesTo.CLIENT).isEmpty())
                cases += it.getTestCasesFor(AppliesTo.SERVER).size
            }
            shape.getTrait(HttpResponseTestsTrait::class.java).orElse(null)?.let {
                check(it.getTestCasesFor(AppliesTo.CLIENT).isEmpty())
                cases += it.getTestCasesFor(AppliesTo.SERVER).size
            }
        }
        check(cases == 163) { "Expected 163 server protocol cases, found $cases" }
    }

    @Test
    fun `client models bind the unified operation sets`() {
        val shared =
            setOf(
                "Greet", "Ping", "NoInputOutput", "Subscribe", "Publish", "StructurePayload",
                "UnionPayload", "ResponseBindings", "RichData", "ModeledDefaults",
            )
        val expected =
            mapOf(
                // Streaming blob uploads are REST-only; sparse and document values are JSON/CBOR-only.
                "restjson" to shared + setOf("Upload", "NullableValues", "DocumentValues"),
                "restxml" to shared + setOf("Upload"),
                "awsjson10" to shared + setOf("NullableValues", "DocumentValues"),
                "awsjson11" to shared + setOf("NullableValues", "DocumentValues"),
                "rpcv2cbor" to shared + setOf("NullableValues"),
            )
        for ((protocol, operationsExpected) in expected) {
            val model =
                Model.assembler().discoverModels()
                    .addImport(models.resolve("multi-protocol-common.smithy").toPath())
                    .addImport(models.resolve("multi-protocol-$protocol.smithy").toPath())
                    .assemble().unwrap()
            val operations = model.expectShape(serviceId, ServiceShape::class.java).operations.map { it.name }.toSet()
            check(operations == operationsExpected) { "$protocol bound $operations" }
        }
    }
}
