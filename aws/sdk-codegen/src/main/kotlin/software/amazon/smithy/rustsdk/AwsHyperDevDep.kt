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
import software.amazon.smithy.rust.codegen.smithy.letIf

val TestedServices = setOf("aws-sdk-kms", "aws-sdk-dynamodb")

class IntegrationTestDecorator : RustCodegenDecorator {
    override val name: String = "IntegrationTest"
    override val order: Byte = 0

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> = baseCustomizations.letIf(TestedServices.contains(protocolConfig.moduleName)) {
        it + AwsHyperDevDep(protocolConfig.runtimeConfig)
    }
}

class AwsHyperDevDep(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection) = when (section) {
        LibRsSection.Body -> writable {
            addDependency(runtimeConfig.awsHyper().copy(scope = DependencyScope.Dev))
            addDependency(Tokio)
        }
        else -> emptySection
    }
}

val Tokio = CargoDependency("tokio", CratesIo("1"), features = listOf("macros", "test-util"), scope = DependencyScope.Dev)
fun RuntimeConfig.awsHyper() = awsRuntimeDependency("aws-hyper", features = listOf("test-util"))
