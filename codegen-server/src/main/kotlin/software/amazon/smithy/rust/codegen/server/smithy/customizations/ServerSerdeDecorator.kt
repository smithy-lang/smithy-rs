/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.serde.extrasCommon
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.hasPublicConstrainedWrapperTupleType

class ServerSerdeDecorator : ServerCodegenDecorator {
    override val name: String = "ServerSerdeDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) = extrasCommon(
        codegenContext,
        rustCrate,
        unwrapConstraints = { shape ->
            writable {
                if (shape.hasPublicConstrainedWrapperTupleType(
                        codegenContext.model,
                        codegenContext.settings.codegenConfig.publicConstrainedTypes,
                    )
                ) {
                    rust(".0")
                }
            }
        },
    )
}
