/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.LibRsCustomization

/**
 * Customizations that apply only to generated clients.
 */
class ClientCustomizations : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "ClientCustomizations"
    override val order: Byte = 0

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations + ClientDocsGenerator()

    override fun supportsCodegenContext(clazz: Class<out CoreCodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}
