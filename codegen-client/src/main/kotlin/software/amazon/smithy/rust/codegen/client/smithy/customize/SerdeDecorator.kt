/*
    * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
    * SPDX-License-Identifier: Apache-2.0
    */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext

class SerdeDecorator: ClientCodegenDecorator {
    override val name: String = "SerdeDecorator"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        codegenContext.settings.
    }
}