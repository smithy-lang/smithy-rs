/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.BytesUtils
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.TempFile
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import java.nio.file.Files
import java.nio.file.Paths

class IntegrationTestDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "IntegrationTest"
    override val order: Byte = 0

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
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
                hasBenches,
            )
        } else {
            baseCustomizations
        }
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
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
        "s3" -> listOf(S3TestDependencies(runtimeConfig))
        else -> emptyList()
    }
}

class TranscribeTestDependencies : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable =
        writable {
            addDependency(AsyncStream)
            addDependency(FuturesCore)
            addDependency(Hound)
        }
}

class S3TestDependencies(
    private val runtimeConfig: RuntimeConfig,
) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable =
        writable {
            addDependency(AsyncStd)
            addDependency(BytesUtils)
            addDependency(Smol)
            addDependency(TempFile)
            runtimeConfig.runtimeCrate("async", scope = DependencyScope.Dev)
            runtimeConfig.runtimeCrate("client", scope = DependencyScope.Dev)
            runtimeConfig.runtimeCrate("http", scope = DependencyScope.Dev)
            runtimeConfig.runtimeCrate("types", scope = DependencyScope.Dev)
        }
}

private val AsyncStd = CargoDependency("async-std", CratesIo("1.12.0"), scope = DependencyScope.Dev)
private val AsyncStream = CargoDependency("async-stream", CratesIo("0.3.0"), DependencyScope.Dev)
private val Criterion = CargoDependency("criterion", CratesIo("0.3.6"), scope = DependencyScope.Dev)
private val FuturesCore = CargoDependency("futures-core", CratesIo("0.3.0"), DependencyScope.Dev)
private val FuturesUtil = CargoDependency("futures-util", CratesIo("0.3.0"), scope = DependencyScope.Dev)
private val Hound = CargoDependency("hound", CratesIo("3.4.0"), DependencyScope.Dev)
private val SerdeJson = CargoDependency("serde_json", CratesIo("1.0.0"), features = emptySet(), scope = DependencyScope.Dev)
private val Smol = CargoDependency("smol", CratesIo("1.2.0"), scope = DependencyScope.Dev)
private val Tokio = CargoDependency("tokio", CratesIo("1.8.4"), features = setOf("macros", "test-util"), scope = DependencyScope.Dev)
private val Tracing = CargoDependency("tracing", CratesIo("0.1.0"), scope = DependencyScope.Dev)
private val TracingSubscriber = CargoDependency("tracing-subscriber", CratesIo("0.3.15"), scope = DependencyScope.Dev, features = setOf("env-filter"))
