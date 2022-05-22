/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

/**
 * Object used *exclusively* in the runtime of the Python server, for separation concerns.
 * Analogous to the companion object in [RuntimeType] and [ServerRuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object PythonServerRuntimeType {

    fun SharedSocket(runtimeConfig: RuntimeConfig) =
        RuntimeType("SharedSocket", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server_python")

    fun Blob(runtimeConfig: RuntimeConfig) =
        RuntimeType("Blob", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server_python::types")

    fun PyError(runtimeConfig: RuntimeConfig) =
        RuntimeType("Error", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig), "${runtimeConfig.crateSrcPrefix}_http_server_python")
}
