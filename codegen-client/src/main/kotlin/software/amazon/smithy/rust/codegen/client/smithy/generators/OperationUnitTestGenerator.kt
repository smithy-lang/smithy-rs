/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

/**
 * Generates operation-level unit tests
 */
class OperationUnitTestGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        // TODO(smithy-rs#3759) don't generate this if there are no customizations adding unit tests.
        writer.rustTemplate(
            """
            ##[cfg(test)]
            mod test {
                #{unit_tests}
            }
            """,
            *preludeScope,
            "unit_tests" to
                writable {
                    writeCustomizations(
                        customizations,
                        OperationSection.UnitTests(customizations, operationShape),
                    )
                },
        )
    }
}
