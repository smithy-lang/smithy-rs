/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import com.moandjiezana.toml.Toml
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.Version
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.generators.CargoTomlGenerator
import software.amazon.smithy.rust.codegen.server.ServerVersion

internal class ServerCargoMetadataTest {
    private fun manifest(server: Boolean): Toml {
        val writer = RustWriter.toml("Cargo.toml")
        CargoTomlGenerator(
            moduleName = "test-service",
            moduleVersion = "1.0.0",
            moduleAuthors = listOf("test"),
            moduleDescription = null,
            moduleLicense = null,
            moduleRepository = null,
            minimumSupportedRustVersion = null,
            protocolId = "aws.protocols#restJson1",
            writer = writer,
            manifestCustomizations = if (server) serverCodegenVersionMetadata() else emptyMap(),
        ).render()
        return Toml().read(writer.toString()).getTable("package.metadata.smithy")
    }

    @Test
    fun `server manifest records artifact version and commit while retaining protocol`() {
        val version = ServerVersion.fromDefaultResource()
        val metadata = manifest(server = true)
        metadata.getString("codegen-version") shouldBe version.codegenVersion
        metadata.getString("codegen-version-commit") shouldBe version.gitHash
        metadata.getString("protocol") shouldBe "aws.protocols#restJson1"
    }

    @Test
    fun `shared manifest retains original commit metadata`() {
        val metadata = manifest(server = false)
        metadata.getString("codegen-version") shouldBe Version.fromDefaultResource().gitHash
        metadata.contains("codegen-version-commit") shouldBe false
    }
}
