/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol

/**
 * [ClientCodegenContext] contains code-generation context that is _specific_ to the [RustClientCodegenPlugin] plugin
 * from the `codegen-client` subproject.
 *
 * It inherits from [CodegenContext], which contains code-generation context that is common to _all_ smithy-rs plugins.
 */
data class ClientCodegenContext(
    override val model: Model,
    override val symbolProvider: RustSymbolProvider,
    override val moduleDocProvider: ModuleDocProvider?,
    override val serviceShape: ServiceShape,
    override val protocol: ShapeId,
    override val settings: ClientRustSettings,
    // Expose the `rootDecorator`, enabling customizations to compose by referencing information from the root codegen
    // decorator
    val rootDecorator: ClientCodegenDecorator,
    val protocolImpl: Protocol? = null,
) : CodegenContext(
    model, symbolProvider, moduleDocProvider, serviceShape, protocol, settings, CodegenTarget.CLIENT,
) {
    val smithyRuntimeMode: SmithyRuntimeMode get() = settings.codegenConfig.enableNewSmithyRuntime
    val enableUserConfigurableRuntimePlugins: Boolean get() = settings.codegenConfig.enableUserConfigurableRuntimePlugins
}
