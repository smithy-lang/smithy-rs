/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.smithy.CodegenConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustCodegenPlugin
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.util.dq
import java.io.File

val TestRuntimeConfig = RuntimeConfig(relativePath = File("../rust-runtime/").absolutePath)
val TestSymbolVisitorConfig = SymbolVisitorConfig(runtimeConfig = TestRuntimeConfig, codegenConfig = CodegenConfig(), handleOptionality = true, handleRustBoxing = true)
fun testSymbolProvider(model: Model): RustSymbolProvider = RustCodegenPlugin.BaseSymbolProvider(model, TestSymbolVisitorConfig)

private const val SmithyVersion = "1.0"
fun String.asSmithyModel(sourceLocation: String? = null): Model {
    val processed = letIf(!this.startsWith("\$version")) { "\$version: ${SmithyVersion.dq()}\n$it" }
    return Model.assembler().discoverModels().addUnparsedModel(sourceLocation ?: "test.smithy", processed).assemble().unwrap()
}
