/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider

object PythonServerRustModule {
    val PythonModuleExport = RustModule.public("python_module_export")
    val PythonOperationAdapter = RustModule.public("python_operation_adaptor")
    val PythonServerApplication = RustModule.public("python_server_application")
}

class PythonServerModuleDocProvider(private val base: ModuleDocProvider) : ModuleDocProvider {
    override fun docs(module: RustModule.LeafModule): String? = when (module) {
        PythonServerRustModule.PythonModuleExport -> "Export PyO3 symbols in the shared library"
        PythonServerRustModule.PythonServerApplication -> "Python server and application implementation."
        // TODO(ServerTeam): Document this module (I don't have context)
        PythonServerRustModule.PythonOperationAdapter -> ""
        else -> base.docs(module)
    }
}
