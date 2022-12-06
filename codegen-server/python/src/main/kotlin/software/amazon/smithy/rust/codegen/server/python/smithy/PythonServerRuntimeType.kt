/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Object used *exclusively* in the runtime of the Python server, for separation concerns.
 * Analogous to the companion object in [RuntimeType] and [ServerRuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object PythonServerRuntimeType {

    fun PySocket(runtimeConfig: RuntimeConfig) =
        RuntimeType("${runtimeConfig.crateSrcPrefix}_http_server_python::PySocket", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig))

    fun Blob(runtimeConfig: RuntimeConfig) =
        RuntimeType("${runtimeConfig.crateSrcPrefix}_http_server_python::types::Blob", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig))

    fun ByteStream(runtimeConfig: RuntimeConfig) =
        RuntimeType("${runtimeConfig.crateSrcPrefix}_http_server_python::types::ByteStream", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig))

    fun DateTime(runtimeConfig: RuntimeConfig) =
        RuntimeType("${runtimeConfig.crateSrcPrefix}_http_server_python::types::DateTime", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig))

    fun PyError(runtimeConfig: RuntimeConfig) =
        RuntimeType("${runtimeConfig.crateSrcPrefix}_http_server_python::Error", PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig))
}
