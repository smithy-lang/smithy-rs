/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import java.io.File

/**
 * Loads the shared multi-protocol client models together with the server extensions. It checks
 * that every expected operation is bound, that server protocol-test cases apply only to servers,
 * and that all cases are reachable from the main or JSON-only document server. It separately checks the client service operation
 * sets so reorganizing the shared models cannot silently drop either side's test coverage.
 */
internal class MultiProtocolServerModelTest {
    private val models = File("../codegen-server-test/custom-test-models/multi-protocol-compat")
    private val serviceId = ShapeId.from("smithy.rust.codegen.test.multiprotocol#MultiProtocolService")

    @ParameterizedTest
    @ValueSource(strings = ["historical", "specs"])
    fun `server extensions retain all server protocol cases`(mode: String) {
        val model =
            Model.assembler().discoverModels()
                .addImport(models.resolve("multi-protocol-common.smithy").toPath())
                .addImport(models.resolve("multi-protocol-server.smithy").toPath())
                .addImport(models.resolve("multi-protocol-awsjson-responses-$mode.smithy").toPath())
                .assemble().unwrap()
        val service = model.expectShape(serviceId, ServiceShape::class.java)
        val operations = service.operations.map { it.name }.toSet()
        val operationsExpected =
            setOf(
                "Greet", "Ping", "NoInputOutput", "Upload", "Subscribe", "Publish", "StructurePayload",
                "UnionPayload", "ResponseBindings", "RichData", "ModeledDefaults", "NullableValues",
            )
        check(operations == operationsExpected) { "Server bound $operations" }
        check(serverCaseCount(model, service) == 139)
    }

    @ParameterizedTest
    @ValueSource(strings = ["historical", "specs"])
    fun `JSON document server binds all 24 document protocol cases`(mode: String) {
        val model =
            Model.assembler().discoverModels()
                .addImport(models.resolve("multi-protocol-common.smithy").toPath())
                .addImport(models.resolve("multi-protocol-document-server.smithy").toPath())
                .addImport(models.resolve("multi-protocol-awsjson-responses-$mode.smithy").toPath())
                .assemble().unwrap()
        val service = model.expectShape(serviceId, ServiceShape::class.java)
        check(service.operations.map { it.name }.toSet() == setOf("DocumentValues"))
        check(serverCaseCount(model, service) == 24)
    }

    private fun serverCaseCount(
        model: Model,
        service: ServiceShape,
    ): Int {
        var cases = 0
        Walker(model).walkShapes(service).forEach { shape ->
            if (shape.id.namespace != serviceId.namespace) return@forEach
            shape.getTrait(HttpRequestTestsTrait::class.java).orElse(null)?.let {
                check(it.getTestCasesFor(AppliesTo.CLIENT).isEmpty())
                check(it.getTestCasesFor(AppliesTo.SERVER).all { test -> test.protocol in service.allTraits })
                cases += it.getTestCasesFor(AppliesTo.SERVER).size
            }
            shape.getTrait(HttpResponseTestsTrait::class.java).orElse(null)?.let {
                check(it.getTestCasesFor(AppliesTo.CLIENT).isEmpty())
                check(it.getTestCasesFor(AppliesTo.SERVER).all { test -> test.protocol in service.allTraits })
                cases += it.getTestCasesFor(AppliesTo.SERVER).size
            }
        }
        return cases
    }

    @Test
    fun `three historical serde paths agree with mainline and specs fixtures preserve inputs`() {
        val cases =
            listOf("historical", "specs").associateWith { mode ->
                val model =
                    Model.assembler().discoverModels()
                        .addImport(models.resolve("multi-protocol-common.smithy").toPath())
                        .addImport(models.resolve("multi-protocol-server.smithy").toPath())
                        .addImport(models.resolve("multi-protocol-awsjson-responses-$mode.smithy").toPath())
                        .assemble().unwrap()
                model.shapes().iterator().asSequence().flatMap { shape ->
                    shape.getTrait(HttpResponseTestsTrait::class.java).orElse(null)
                        ?.getTestCasesFor(AppliesTo.SERVER).orEmpty().asSequence()
                }.filter { it.protocol.toString().startsWith("aws.protocols#awsJson") }
                    .associateBy { it.id }
            }
        val historical = cases.getValue("historical")
        val specs = cases.getValue("specs")
        check(historical.size == 36 && historical.keys == specs.keys)
        val reference =
            Node.parse(models.resolve("awsjson-historical-observations.json").readText()).expectObjectNode()
        check(reference.expectStringMember("referenceCheckout").value == "mainline")
        check(reference.expectStringMember("referenceRevision").value.matches(Regex("[0-9a-f]{40}")))
        val observations = reference.expectArrayMember("cases").elements
        check(observations.size == historical.size)
        for (node in observations) {
            val observation = node.expectObjectNode()
            val id = observation.expectStringMember("id").value
            val legacy = historical.getValue(id)
            val spec = specs.getValue(id)
            check(legacy.params == spec.params && legacy.protocol == spec.protocol) { "Different inputs for $id" }
            check(legacy.protocol.toString() == observation.expectStringMember("protocol").value)
            check(legacy.code == observation.expectNumberMember("code").value.toInt()) { "Legacy status for $id" }
            check(Node.parse(legacy.body.get()) == Node.parse(observation.expectStringMember("body").value)) {
                "Legacy body for $id"
            }
            for (path in listOf("schemaSerdeFalse", "schemaSerdeHistorical")) {
                val observed = observation.expectObjectMember(path)
                check(observed.expectNumberMember("code") == observation.expectNumberMember("code"))
                check(observed.expectObjectMember("headers") == observation.expectObjectMember("headers"))
                check(
                    Node.parse(observed.expectStringMember("body").value) ==
                        Node.parse(observation.expectStringMember("body").value),
                ) { "$path differs from mainline for $id" }
            }
            for ((key, value) in observation.expectObjectMember("headers").members) {
                if (key.value == "content-length") {
                    check(key.value in legacy.requireHeaders)
                } else {
                    check(legacy.headers[key.value] == value.expectStringNode().value) { "Legacy header for $id" }
                }
            }
            check(spec.code == if (id.endsWith("RichFailureError")) 400 else 200) { "Specs status for $id" }
            val observedSpec = observation.expectObjectMember("smithySpecs")
            check(observedSpec.expectNumberMember("code").value.toInt() == spec.code)
            check(Node.parse(observedSpec.expectStringMember("body").value) == Node.parse(spec.body.get()))
            val observedHeaderNames = observedSpec.expectObjectMember("headers").members.keys.map { it.value }.toSet()
            check(observedHeaderNames == spec.headers.keys + spec.requireHeaders + "content-length")
            check(spec.forbidHeaders.none { it in observedHeaderNames })
        }
        for (version in listOf("10", "11")) {
            for (mode in cases.values) {
                val absent = mode.getValue("MultiAwsjson${version}DocumentValuesAbsent")
                val explicitNull = mode.getValue("MultiAwsjson${version}DocumentValuesOverride")
                check(absent.params.members.isEmpty())
                check(explicitNull.params.expectMember("document").isNullNode)
                check(Node.parse(absent.body.get()) == Node.parse("{}"))
                check(Node.parse(explicitNull.body.get()) == Node.parse("""{"document":null}"""))
            }
        }
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
