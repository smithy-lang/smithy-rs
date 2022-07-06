/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.server.smithy.RustCodegenServerPlugin
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider

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

fun serverTestRustSettings(
    service: ShapeId = ShapeId.from("notrelevant#notrelevant"),
    moduleName: String = "test-module",
    moduleVersion: String = "0.0.1",
    moduleAuthors: List<String> = listOf("notrelevant"),
    moduleDescription: String = "not relevant",
    moduleRepository: String? = null,
    runtimeConfig: RuntimeConfig = TestRuntimeConfig,
    codegenConfig: ServerCodegenConfig = ServerCodegenConfig(),
    license: String? = null,
    examplesUri: String? = null,
    customizationConfig: ObjectNode? = null
) = ServerRustSettings(
    service,
    moduleName,
    moduleVersion,
    moduleAuthors,
    moduleDescription,
    moduleRepository,
    runtimeConfig,
    codegenConfig,
    license,
    examplesUri,
    customizationConfig
)

fun serverTestCodegenContext(
    model: Model,
    serviceShape: ServiceShape? = null,
    settings: ServerRustSettings = serverTestRustSettings(),
    protocolShapeId: ShapeId? = null
): ServerCodegenContext = ServerCodegenContext(
    model,
    testSymbolProvider(model),
    serviceShape
        ?: model.serviceShapes.firstOrNull()
        ?: ServiceShape.builder().version("test").id("test#Service").build(),
    protocolShapeId ?: ShapeId.from("test#Protocol"),
    settings,
)
