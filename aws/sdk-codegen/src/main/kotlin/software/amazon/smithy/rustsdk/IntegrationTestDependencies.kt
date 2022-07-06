/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import java.nio.file.Files
import java.nio.file.Paths

class IntegrationTestDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "IntegrationTest"
    override val order: Byte = 0

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        val integrationTestPath = Paths.get(SdkSettings.from(codegenContext.settings).integrationTestPath)
        check(Files.exists(integrationTestPath)) {
            "Failed to find the AWS SDK integration tests. Make sure the integration test path is configured " +
                "correctly in the smithy-build.json."
        }

        val moduleName = codegenContext.moduleName.substring("aws-sdk-".length)
        val testPackagePath = integrationTestPath.resolve(moduleName)
        return if (Files.exists(testPackagePath) && Files.exists(testPackagePath.resolve("Cargo.toml"))) {
            val hasTests = Files.exists(testPackagePath.resolve("tests"))
            val hasBenches = Files.exists(testPackagePath.resolve("benches"))
            baseCustomizations + IntegrationTestDependencies(
                moduleName,
                codegenContext.runtimeConfig,
                hasTests,
                hasBenches
            )
        } else {
            baseCustomizations
        }
    }
}

class IntegrationTestDependencies(
    private val moduleName: String,
    private val runtimeConfig: RuntimeConfig,
    private val hasTests: Boolean,
    private val hasBenches: Boolean,
) : LibRsCustomization() {
    override fun section(section: LibRsSection) = when (section) {
        is LibRsSection.Body -> writable {
            if (hasTests) {
                val smithyClient = CargoDependency.SmithyClient(runtimeConfig)
                    .copy(features = setOf("test-util"), scope = DependencyScope.Dev)
                addDependency(smithyClient)
                addDependency(CargoDependency.SmithyProtocolTestHelpers(runtimeConfig))
                addDependency(SerdeJson)
                addDependency(Tokio)
                addDependency(FuturesUtil)
                addDependency(Tracing)
                addDependency(TracingSubscriber)
            }
            if (hasBenches) {
                addDependency(Criterion)
            }
            for (serviceSpecific in serviceSpecificCustomizations()) {
                serviceSpecific.section(section)(this)
            }
        }
        else -> emptySection
    }

    private fun serviceSpecificCustomizations(): List<LibRsCustomization> = when (moduleName) {
        "transcribestreaming" -> listOf(TranscribeTestDependencies())
        else -> emptyList()
    }
}

class TranscribeTestDependencies : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable = writable {
        addDependency(AsyncStream)
        addDependency(FuturesCore)
        addDependency(Hound)
    }
}

private val AsyncStream = CargoDependency("async-stream", CratesIo("0.3"), DependencyScope.Dev)
private val Criterion = CargoDependency("criterion", CratesIo("0.3"), scope = DependencyScope.Dev)
private val FuturesCore = CargoDependency("futures-core", CratesIo("0.3"), DependencyScope.Dev)
private val Hound = CargoDependency("hound", CratesIo("3.4"), DependencyScope.Dev)
private val SerdeJson = CargoDependency("serde_json", CratesIo("1"), features = emptySet(), scope = DependencyScope.Dev)
private val Tokio = CargoDependency("tokio", CratesIo("1"), features = setOf("macros", "test-util"), scope = DependencyScope.Dev)
private val FuturesUtil = CargoDependency("futures-util", CratesIo("0.3"), scope = DependencyScope.Dev)
private val Tracing = CargoDependency("tracing", CratesIo("0.1"), scope = DependencyScope.Dev)
private val TracingSubscriber = CargoDependency("tracing-subscriber", CratesIo("0.2"), scope = DependencyScope.Dev)
