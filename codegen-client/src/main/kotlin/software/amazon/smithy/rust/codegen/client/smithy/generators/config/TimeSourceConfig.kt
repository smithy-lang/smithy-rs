/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection

fun timeSourceCustomization(codegenContext: ClientCodegenContext) = standardConfigParam(
    ConfigParam(
        "time_source",
        RuntimeType.smithyAsync(codegenContext.runtimeConfig).resolve("time::SharedTimeSource").toSymbol(),
        setterDocs = writable { docs("""Sets the time source used for this service""") },
        optional = false,
    ),
)

class TimeSourceOperationCustomization : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rust(
                    """
                    ${section.request}.properties_mut().insert(${section.config}.time_source.clone());
                    """,
                )
            }

            else -> emptySection
        }
    }
}
