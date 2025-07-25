/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider

object TsServerRustModule {
    val TsOperationAdapter = RustModule.public("ts_operation_adaptor")
    val TsServerApplication = RustModule.public("ts_server_application")
}

class TsServerModuleDocProvider(private val base: ModuleDocProvider) : ModuleDocProvider {
    override fun docsWriter(module: RustModule.LeafModule): Writable? {
        val strDoc: (String) -> Writable = { str -> writable { docs(str) } }
        return when (module) {
            TsServerRustModule.TsServerApplication -> strDoc("Ts server and application implementation.")
            // TODO(ServerTeam): Document this module (I don't have context)
            TsServerRustModule.TsOperationAdapter -> null
            else -> base.docsWriter(module)
        }
    }
}
