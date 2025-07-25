/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.testutil

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.python.smithy.customizations.DECORATORS
import software.amazon.smithy.rust.codegen.server.smithy.customizations.CustomValidationExceptionWithReasonDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.ServerRequiredCustomizations
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.CombinedServerCodegenDecorator
import java.io.File
import java.nio.file.Path

val TestRuntimeConfig =
    RuntimeConfig(runtimeCrateLocation = RuntimeCrateLocation.path(File("../../rust-runtime").absolutePath))

fun generatePythonServerPluginContext(model: Model) = generatePluginContext(model, runtimeConfig = TestRuntimeConfig)

fun executePythonServerCodegenVisitor(pluginCtx: PluginContext) {
    val codegenDecorator =
        CombinedServerCodegenDecorator.fromClasspath(
            pluginCtx,
            *DECORATORS,
            ServerRequiredCustomizations(),
            SmithyValidationExceptionDecorator(),
            CustomValidationExceptionWithReasonDecorator(),
        )
    PythonServerCodegenVisitor(pluginCtx, codegenDecorator).execute()
}

fun cargoTest(workdir: Path) =
    // `--no-default-features` is required to disable `pyo3/extension-module` which causes linking errors
    // see `PyO3ExtensionModuleDecorator`'s comments fore more detail.
    "cargo test --no-default-features --no-fail-fast".runCommand(
        workdir,
        mapOf(
            // Those are required to run tests on macOS, see: https://pyo3.rs/main/building_and_distribution#macos
            "CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS" to "-C link-arg=-undefined -C link-arg=dynamic_lookup",
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS" to "-C link-arg=-undefined -C link-arg=dynamic_lookup",
        ),
    )
