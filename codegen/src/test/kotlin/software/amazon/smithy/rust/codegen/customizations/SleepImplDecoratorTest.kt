/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.customizations.SleepImplProviderConfig
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.rustSettings
import software.amazon.smithy.rust.codegen.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.testutil.validateConfigCustomizations

internal class SleepImplDecoratorTest {
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
        val model = RecursiveShapeBoxer.transform(OperationNormalizer.transform(baseModel))
        val project = TestWorkspace.testProject()
        val codegenContext = testCodegenContext(model, settings = project.rustSettings())

        validateConfigCustomizations(SleepImplProviderConfig(codegenContext), project)
    }
}
