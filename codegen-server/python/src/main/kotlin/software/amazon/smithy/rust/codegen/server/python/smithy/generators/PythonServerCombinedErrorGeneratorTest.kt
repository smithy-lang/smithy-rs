/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.lookup

class PythonServerCombinedErrorGeneratorTest {
    private val baseModel = """"
        namespace error

        operation TestOperation {
            errors: [TestOperationError]
        }

        @error("server")
        structure TestOperationError {
            message: String
        }
    """.asSmithyModel()
    private val model = OperationNormalizer.transform(baseModel)
    private val symbolProvider = serverTestSymbolProvider(model)

    @Test
    fun `it generates From pyo3 PyErr for TestOperationError`() {
        val project = TestWorkspace.testProject(symbolProvider)
        project.withModule(RustModule.public("error")) { writer ->
            val generator = PythonServerCombinedErrorGenerator(model, symbolProvider, model.lookup("error#TestOperation"))
            generator.render(writer)
        }
    }
}
