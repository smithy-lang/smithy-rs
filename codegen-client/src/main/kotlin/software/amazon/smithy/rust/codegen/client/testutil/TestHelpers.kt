/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenConfig
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientModuleProvider
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.RustClientCodegenPlugin
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestModuleDocProvider
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWriterDelegator
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

fun testClientRustSettings(
    service: ShapeId = ShapeId.from("notrelevant#notrelevant"),
    moduleName: String = "test-module",
    moduleVersion: String = "0.0.1",
    moduleAuthors: List<String> = listOf("notrelevant"),
    moduleDescription: String = "not relevant",
    moduleRepository: String? = null,
    runtimeConfig: RuntimeConfig = TestRuntimeConfig,
    codegenConfig: ClientCodegenConfig = ClientCodegenConfig(),
    license: String? = null,
    examplesUri: String? = null,
    minimumSupportedRustVersion: String? = null,
    hintMostlyUnused: Boolean = false,
    customizationConfig: ObjectNode? = null,
) = ClientRustSettings(
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
    minimumSupportedRustVersion,
    hintMostlyUnused,
    customizationConfig,
)

val TestClientRustSymbolProviderConfig =
    RustSymbolProviderConfig(
        runtimeConfig = TestRuntimeConfig,
        renameExceptions = true,
        nullabilityCheckMode = NullableIndex.CheckMode.CLIENT_ZERO_VALUE_V1,
        moduleProvider = ClientModuleProvider,
    )

private class ClientTestCodegenDecorator : ClientCodegenDecorator {
    override val name = "test"
    override val order: Byte = 0
}

fun testSymbolProvider(
    model: Model,
    serviceShape: ServiceShape? = null,
): RustSymbolProvider =
    RustClientCodegenPlugin.baseSymbolProvider(
        testClientRustSettings(),
        model,
        serviceShape ?: ServiceShape.builder().version("test").id("test#Service").build(),
        TestClientRustSymbolProviderConfig,
        ClientTestCodegenDecorator(),
    )

fun testClientCodegenContext(
    model: Model = "namespace empty".asSmithyModel(),
    symbolProvider: RustSymbolProvider? = null,
    serviceShape: ServiceShape? = null,
    settings: ClientRustSettings = testClientRustSettings(),
    rootDecorator: ClientCodegenDecorator? = null,
): ClientCodegenContext =
    ClientCodegenContext(
        model,
        symbolProvider ?: testSymbolProvider(model),
        TestModuleDocProvider,
        serviceShape
            ?: model.serviceShapes.firstOrNull()
            ?: ServiceShape.builder().version("test").id("test#Service").build(),
        ShapeId.from("test#Protocol"),
        settings,
        rootDecorator ?: CombinedClientCodegenDecorator(emptyList()),
    )

fun ClientCodegenContext.withEnableUserConfigurableRuntimePlugins(
    enableUserConfigurableRuntimePlugins: Boolean,
): ClientCodegenContext =
    copy(settings = settings.copy(codegenConfig = settings.codegenConfig.copy(enableUserConfigurableRuntimePlugins = enableUserConfigurableRuntimePlugins)))

fun TestWriterDelegator.clientRustSettings() =
    testClientRustSettings(
        service = ShapeId.from("fake#Fake"),
        moduleName = "test_${baseDir.toFile().nameWithoutExtension}",
        codegenConfig = codegenConfig as ClientCodegenConfig,
    )

fun TestWriterDelegator.clientCodegenContext(model: Model) =
    testClientCodegenContext(model, settings = clientRustSettings())
