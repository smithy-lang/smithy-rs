/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * Names shared by operation declarations and their references.
 *
 * Preserve the legacy marker spelling and the schema descriptor spelling independently:
 * PascalCase followed by snake_case is not equivalent to snake_case for names such as
 * `ACLsUpdateStatus`. Derive both from the original symbol, which also carries service renames.
 */
internal class ServerOperationNames(symbol: Symbol) {
    val markerType: String = symbol.name.toPascalCase()
    val schemaConst: String = "OPERATION_" + symbol.name.toSnakeCase().uppercase()
}
