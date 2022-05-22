/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.customizations

import software.amazon.smithy.rust.codegen.server.smithy.customizations.AddInternalServerErrorToAllOpsDecorator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ManifestCustomizations

val DECORATORS = listOf(AddInternalServerErrorToAllOpsDecorator(), CdylibDecorator())

class CdylibDecorator : RustCodegenDecorator {
    override val name: String = "CdylibDecorator"
    override val order: Byte = 0

    override fun crateManifestCustomizations(
        codegenContext: CodegenContext
    ): ManifestCustomizations =
        mapOf("lib" to mapOf("name" to codegenContext.settings.moduleName, "crate-type" to listOf("cdylib")))
}

class PythonServerCodegenDecorator : CombinedCodegenDecorator(DECORATORS) {
    override val name: String = "PythonServerCodegenDecorator"
    override val order: Byte = -1
}
