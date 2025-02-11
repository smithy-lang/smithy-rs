/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Approx
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.AsyncStd
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.AsyncStream
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.BytesUtils
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Criterion
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.FastRand
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.FuturesCore
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.FuturesUtil
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.HdrHistogram
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Hound
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.HttpBody
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.SerdeJson
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Smol
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.TempFile
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Tokio
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Tracing
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.TracingAppender
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.TracingSubscriber
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.TracingTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.smithyExperimental
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.smithyProtocolTestHelpers
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.smithyRuntime
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.smithyRuntimeApiTestUtil
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.testutil.testDependenciesOnly
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations
import software.amazon.smithy.rustsdk.AwsCargoDependency.awsConfig
import software.amazon.smithy.rustsdk.AwsCargoDependency.awsRuntime
import java.nio.file.Files
import java.nio.file.Paths
import kotlin.io.path.absolute

class IntegrationTestDecorator : ClientCodegenDecorator {
    override val name: String = "IntegrationTest"
    override val order: Byte = 0

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        val integrationTestPath = Paths.get(SdkSettings.from(codegenContext.settings).integrationTestPath)
        check(Files.exists(integrationTestPath)) {
            "Failed to find the AWS SDK integration tests (${integrationTestPath.absolute()}). Make sure the integration test path is configured " +
                "correctly in the smithy-build.json."
        }

        val moduleName = codegenContext.moduleName.removePrefix("aws-sdk-")
        val testPackagePath = integrationTestPath.resolve(moduleName)
        return if (Files.exists(testPackagePath) && Files.exists(testPackagePath.resolve("Cargo.toml"))) {
            val hasTests = Files.exists(testPackagePath.resolve("tests"))
            val hasBenches = Files.exists(testPackagePath.resolve("benches"))
            baseCustomizations +
                IntegrationTestDependencies(
                    codegenContext,
                    moduleName,
                    hasTests,
                    hasBenches,
                )
        } else {
            baseCustomizations
        }
    }
}

class IntegrationTestDependencies(
    codegenContext: ClientCodegenContext,
    private val moduleName: String,
    private val hasTests: Boolean,
    private val hasBenches: Boolean,
) : LibRsCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val serviceShape = codegenContext.serviceShape
    private val model = codegenContext.model

    override fun section(section: LibRsSection) =
        when (section) {
            is LibRsSection.Body ->
                testDependenciesOnly {
                    if (hasTests) {
                        val smithyAsync =
                            CargoDependency.smithyAsync(runtimeConfig)
                                .copy(features = setOf("test-util"), scope = DependencyScope.Dev)
                        val smithyTypes =
                            CargoDependency.smithyTypes(runtimeConfig)
                                .copy(features = setOf("test-util"), scope = DependencyScope.Dev)
                        addDependency(awsRuntime(runtimeConfig).toDevDependency().withFeature("test-util"))
                        addDependency(FuturesUtil)
                        addDependency(SerdeJson)
                        addDependency(smithyAsync)
                        addDependency(smithyProtocolTestHelpers(runtimeConfig))
                        addDependency(smithyRuntime(runtimeConfig).copy(features = setOf("test-util", "wire-mock"), scope = DependencyScope.Dev))
                        addDependency(smithyRuntimeApiTestUtil(runtimeConfig))
                        addDependency(smithyTypes)
                        addDependency(Tokio)
                        addDependency(Tracing.toDevDependency())
                        addDependency(TracingSubscriber)
                    }
                    if (hasBenches) {
                        addDependency(Criterion)
                    }
                    if (serviceShape.hasEventStreamOperations(model)) {
                        addDependency(
                            CargoDependency.smithyEventStream(runtimeConfig)
                                .copy(features = setOf("test-util"), scope = DependencyScope.Dev),
                        )
                    }
                    for (serviceSpecific in serviceSpecificCustomizations()) {
                        serviceSpecific.section(section)(this)
                    }
                }

            else -> emptySection
        }

    private fun serviceSpecificCustomizations(): List<LibRsCustomization> =
        when (moduleName) {
            "transcribestreaming" -> listOf(TranscribeTestDependencies(runtimeConfig))
            "s3" -> listOf(S3TestDependencies(runtimeConfig))
            "dynamodb" -> listOf(DynamoDbTestDependencies())
            else -> emptyList()
        }
}

class TranscribeTestDependencies(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable =
        writable {
            addDependency(AsyncStream)
            addDependency(FuturesCore)
            addDependency(Hound)
        }
}

class DynamoDbTestDependencies : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable =
        writable {
            addDependency(Approx)
        }
}

class S3TestDependencies(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable =
        writable {
            addDependency(awsConfig(runtimeConfig).toDevDependency().withFeature("behavior-version-latest"))
            addDependency(smithyExperimental(runtimeConfig).toDevDependency())
            addDependency(AsyncStd)
            addDependency(BytesUtils.toDevDependency())
            addDependency(FastRand.toDevDependency())
            addDependency(FuturesUtil.toDevDependency())
            addDependency(HdrHistogram)
            addDependency(HttpBody.toDevDependency())
            addDependency(Smol)
            addDependency(TempFile)
            addDependency(TracingAppender)
            addDependency(TracingTest)
        }
}
