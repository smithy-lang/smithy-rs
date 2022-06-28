/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.server.smithy.RustCodegenServerPlugin
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig

// These are the settings we default to if the user does not override them in their `smithy-build.json`.
val ServerTestSymbolVisitorConfig = SymbolVisitorConfig(
    runtimeConfig = TestRuntimeConfig,
    renameExceptions = false,
    handleRustBoxing = true,
    handleRequired = true
)

fun serverTestSymbolProvider(
    model: Model,
    serviceShape: ServiceShape? = null,
): RustSymbolProvider =
    RustCodegenServerPlugin.baseSymbolProvider(
        model,
        serviceShape ?: ServiceShape.builder().version("test").id("test#Service").build(),
        ServerTestSymbolVisitorConfig
    )
