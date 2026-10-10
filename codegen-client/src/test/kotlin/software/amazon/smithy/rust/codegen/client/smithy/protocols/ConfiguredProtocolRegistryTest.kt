/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig

class ConfiguredProtocolRegistryTest {
    private fun schemaLine(
        name: String,
        version: String,
        alias: Boolean,
    ) = ConfiguredProtocolRepresentation(
        payloadType =
            CargoDependency(
                name,
                CratesIo(version),
                `package` = if (alias) "aws-smithy-schema" else null,
            ).toType().resolve("protocol::SchemaProtocol"),
        adapt = writable { rust("protocol.v1()") },
    )

    @Test
    fun `default registry is valid and covers this client's schema line`() {
        val representations = ConfiguredProtocolRegistry.representations(TestRuntimeConfig)
        ConfiguredProtocolRegistry.validate(representations)
        representations.map { (it.payloadType.dependency as CargoDependency).name } shouldBe
            listOf(CargoDependency.smithySchema(TestRuntimeConfig).name)
    }

    @Test
    fun `aliased compatibility lines are accepted and emitted as Cargo package aliases`() {
        val v2 = schemaLine("aws-smithy-schema-v2", "2", alias = true)
        ConfiguredProtocolRegistry.validate(listOf(schemaLine("aws-smithy-schema", "1", alias = false), v2))

        // The generated manifest entry is `aws-smithy-schema-v2 = { version = "2", package = "aws-smithy-schema" }`.
        val manifestEntry = (v2.payloadType.dependency as CargoDependency).toMap()
        manifestEntry["package"] shouldBe "aws-smithy-schema"
        manifestEntry["version"] shouldBe "2"
        v2.payloadType.fullyQualifiedName() shouldBe "::aws_smithy_schema_v2::protocol::SchemaProtocol"
    }

    @Test
    fun `two compatibility lines under one dependency name are rejected`() {
        val err =
            assertThrows<IllegalStateException> {
                ConfiguredProtocolRegistry.validate(
                    listOf(
                        schemaLine("aws-smithy-schema", "1", alias = false),
                        schemaLine("aws-smithy-schema", "2", alias = false),
                    ),
                )
            }
        err.message!! shouldContain "registered more than once"
    }

    @Test
    fun `one dependency name at two versions is rejected`() {
        val v2UnderOriginalName =
            ConfiguredProtocolRepresentation(
                payloadType =
                    CargoDependency("aws-smithy-schema", CratesIo("2"))
                        .toType().resolve("v2::protocol::SchemaProtocol"),
                adapt = writable { rust("protocol.v1()") },
            )
        val err =
            assertThrows<IllegalStateException> {
                ConfiguredProtocolRegistry.validate(
                    listOf(schemaLine("aws-smithy-schema", "1", alias = false), v2UnderOriginalName),
                )
            }
        err.message!! shouldContain "several versions"
    }

    @Test
    fun `empty registry is rejected`() {
        assertThrows<IllegalStateException> { ConfiguredProtocolRegistry.validate(emptyList()) }
    }
}
