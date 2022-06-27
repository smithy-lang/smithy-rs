/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget

/**
 * [ServerCodegenContext] contains code-generation context that is _specific_ to the [RustCodegenServerPlugin] plugin
 * from the `rust-codegen-server` subproject.
 *
 * It inherits from [CoreCodegenContext], which contains code-generation context that is common to _all_ smithy-rs plugins.
 *
 * This class has to live in the `codegen` subproject because it is referenced in common generators to both client
 * and server (like [JsonParserGenerator]).
 */
data class ServerCodegenContext(
    override val model: Model,
    override val symbolProvider: RustSymbolProvider,
    override val serviceShape: ServiceShape,
    override val protocol: ShapeId,
    override val settings: ServerRustSettings,
) : CoreCodegenContext(
    model, symbolProvider, serviceShape, protocol, settings, CodegenTarget.SERVER
)
