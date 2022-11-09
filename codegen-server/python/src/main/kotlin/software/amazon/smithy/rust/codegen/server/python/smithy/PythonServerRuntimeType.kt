/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency.smithyHttpServerPython

/**
 * Object used *exclusively* in the runtime of the Python server, for separation concerns.
 * Analogous to the companion object in [RuntimeType] and [ServerRuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object PythonServerRuntimeType {

    fun PySocket(runtimeConfig: RuntimeConfig) = smithyHttpServerPython(runtimeConfig).asType().resolve("PySocket")

    fun Blob(runtimeConfig: RuntimeConfig) = smithyHttpServerPython(runtimeConfig).asType().resolve("types::Blob")

    fun ByteStream(runtimeConfig: RuntimeConfig) = smithyHttpServerPython(runtimeConfig).asType().resolve("types::ByteStream")

    fun DateTime(runtimeConfig: RuntimeConfig) = smithyHttpServerPython(runtimeConfig).asType().resolve("types::DateTime")

    fun PyError(runtimeConfig: RuntimeConfig) = smithyHttpServerPython(runtimeConfig).asType().resolve("Error")
}
