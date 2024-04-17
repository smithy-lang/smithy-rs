/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.customizations

import software.amazon.smithy.rust.codegen.client.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.generators.LibRsCustomization

/**
 * Customizations that apply only to generated clients.
 */
class ClientCustomizations : ClientCodegenDecorator {
    override val name: String = "ClientCustomizations"
    override val order: Byte = 0

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations + ClientDocsGenerator(codegenContext)
}
