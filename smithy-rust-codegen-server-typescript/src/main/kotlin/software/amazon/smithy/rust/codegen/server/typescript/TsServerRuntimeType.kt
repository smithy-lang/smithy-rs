/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript

import software.amazon.smithy.rust.codegen.core.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.RuntimeType

/**
 * Object used *exclusively* in the runtime of the Node server, for separation concerns.
 * Analogous to the companion object in [RuntimeType] and [software.amazon.smithy.rust.codegen.server.ServerRuntimeType]; see its documentation for details.
 * For a runtime type that is used in the client, or in both the client and the server, use [RuntimeType] directly.
 */
object TsServerRuntimeType {
    fun blob(runtimeConfig: RuntimeConfig) =
        TsServerCargoDependency.smithyHttpServerTs(runtimeConfig).toType().resolve("types::Blob")

    fun byteStream(runtimeConfig: RuntimeConfig) =
        TsServerCargoDependency.smithyHttpServerTs(runtimeConfig).toType().resolve("types::ByteStream")

    fun dateTime(runtimeConfig: RuntimeConfig) =
        TsServerCargoDependency.smithyHttpServerTs(runtimeConfig).toType().resolve("types::DateTime")

    fun document(runtimeConfig: RuntimeConfig) =
        TsServerCargoDependency.smithyHttpServerTs(runtimeConfig).toType().resolve("types::Document")
}
