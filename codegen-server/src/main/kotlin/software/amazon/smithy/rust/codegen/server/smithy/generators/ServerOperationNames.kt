/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

/**
 * Names shared by operation declarations and their references.
 *
 * Preserve the legacy marker spelling while encoding schema names injectively.
 * Case folding and suffixes such as `_ERRORS` must not make descriptors collide
 * with other descriptors or their auxiliary statics. The symbol carries service renames.
 */
internal class ServerOperationNames(symbol: Symbol) {
    val markerType: String = symbol.name.toPascalCase()
    val schemaConst: String =
        "OPERATION_" + symbol.name.toByteArray(Charsets.UTF_8).joinToString("") { "%02X".format(it) }
}
