/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider

object PythonServerRustModule {
    val PythonModuleExport = RustModule.public("python_module_export")
    val PythonOperationAdapter = RustModule.public("python_operation_adaptor")
    val PythonServerApplication = RustModule.public("python_server_application")
    val PythonEventStream = RustModule.public("python_event_stream")
}

class PythonServerModuleDocProvider(private val base: ModuleDocProvider) : ModuleDocProvider {
    override fun docsWriter(module: RustModule.LeafModule): Writable? {
        val strDoc: (String) -> Writable = { str -> writable { docs(str) } }
        return when (module) {
            PythonServerRustModule.PythonModuleExport -> strDoc("Export PyO3 symbols in the shared library")
            PythonServerRustModule.PythonServerApplication -> strDoc("Python server and application implementation.")
            PythonServerRustModule.PythonOperationAdapter -> strDoc("Operation adapters that delegate to Python handlers.")
            PythonServerRustModule.PythonEventStream -> strDoc("Python wrapper types for event streams.")
            else -> base.docsWriter(module)
        }
    }
}
