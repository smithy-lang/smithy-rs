/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerServiceSchemaGenerator

/**
 * Generates the `crate::schema` service and operation descriptors when the `schemaSerde` codegen setting is on.
 */
class ServerSchemaDecorator : ServerCodegenDecorator {
    override val name: String = "ServerSchemaDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (codegenContext.settings.codegenConfig.schemaSerde) {
            ServerServiceSchemaGenerator(codegenContext).render(rustCrate)
        }
    }
}
