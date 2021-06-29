/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import java.nio.file.Files
import java.nio.file.Paths

class IntegrationTestDecorator : RustCodegenDecorator {
    override val name: String = "IntegrationTest"
    override val order: Byte = 0

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        val integrationTestPath = Paths.get("aws/sdk/integration-tests")
        check(Files.exists(integrationTestPath)) {
            "IntegrationTestDecorator expects to be run from the smithy-rs package root"
        }

        val testPackagePath = integrationTestPath.resolve(protocolConfig.moduleName.substring("aws-sdk-".length))
        return if (Files.exists(testPackagePath) && Files.exists(testPackagePath.resolve("Cargo.toml"))) {
            val hasTests = Files.exists(testPackagePath.resolve("tests"))
            val hasBenches = Files.exists(testPackagePath.resolve("benches"))
            baseCustomizations + IntegrationTestDependencies(protocolConfig.runtimeConfig, hasTests, hasBenches)
        } else {
            baseCustomizations
        }
    }
}

class IntegrationTestDependencies(
    private val runtimeConfig: RuntimeConfig,
    private val hasTests: Boolean,
    private val hasBenches: Boolean,
) : LibRsCustomization() {
    override fun section(section: LibRsSection) = when (section) {
        LibRsSection.Body -> writable {
            if (hasTests) {
                addDependency(runtimeConfig.awsHyper().copy(scope = DependencyScope.Dev))
                addDependency(SerdeJson)
                addDependency(Tokio)
            }
            if (hasBenches) {
                addDependency(Criterion)
            }
        }
        else -> emptySection
    }
}

val Criterion = CargoDependency("criterion", CratesIo("0.3"), scope = DependencyScope.Dev)
val SerdeJson = CargoDependency("serde_json", CratesIo("1"), features = emptyList())
val Tokio = CargoDependency("tokio", CratesIo("1"), features = listOf("macros", "test-util"), scope = DependencyScope.Dev)
fun RuntimeConfig.awsHyper() = awsRuntimeDependency("aws-hyper", features = listOf("test-util"))
