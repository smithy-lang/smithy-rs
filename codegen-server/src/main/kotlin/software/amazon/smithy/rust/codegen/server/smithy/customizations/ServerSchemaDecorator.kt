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
 * Generates the `crate::schema` service and operation descriptors for the schema request path.
 *
 * The descriptors are `aws-smithy-http-server` types read by its schema router, so they are only generated when
 * the service is built on that path (the `schemaSerde` codegen setting on HTTP 1.x).
 */
class ServerSchemaDecorator : ServerCodegenDecorator {
    override val name: String = "ServerSchemaDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (codegenContext.usesSchemaHttpSerde) {
            ServerServiceSchemaGenerator(codegenContext).render(rustCrate)
        }
    }
}
