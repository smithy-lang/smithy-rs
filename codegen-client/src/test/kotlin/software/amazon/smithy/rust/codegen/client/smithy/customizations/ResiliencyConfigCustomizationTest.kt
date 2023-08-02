/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenConfig
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginGenerator
import software.amazon.smithy.rust.codegen.client.testutil.clientRustSettings
import software.amazon.smithy.rust.codegen.client.testutil.stubConfigProject
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest

internal class ResiliencyConfigCustomizationTest {
    private val baseModel = """
        namespace test
        use aws.protocols#awsQuery

        structure SomeOutput {
            @xmlAttribute
            someAttribute: Long,

            someVal: String
        }

        operation SomeOperation {
            output: SomeOutput
        }
    """.asSmithyModel()

    @Test
    fun `generates a valid config`() {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val project = TestWorkspace.testProject(model, ClientCodegenConfig())
        val codegenContext = testClientCodegenContext(model, settings = project.clientRustSettings())

        stubConfigProject(codegenContext, ResiliencyConfigCustomization(codegenContext), project)
        project.withModule(ClientRustModule.config) {
            ServiceRuntimePluginGenerator(codegenContext).render(
                this,
                listOf(ResiliencyServiceRuntimePluginCustomization(codegenContext)),
            )
        }
        ResiliencyReExportCustomization(codegenContext).extras(project)
        project.compileAndTest()
    }
}
