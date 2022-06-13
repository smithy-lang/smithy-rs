/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.codegen.core.ImportContainer
import software.amazon.smithy.codegen.core.Symbol

class UseDeclarations(private val namespace: String) : ImportContainer {
    private val imports: MutableSet<UseStatement> = mutableSetOf()
    fun addImport(moduleName: String, symbolName: String, alias: String = symbolName) {
        imports.add(UseStatement(moduleName, symbolName, alias))
    }

    override fun toString(): String {
        return imports.map { it.toString() }.sorted().joinToString(separator = "\n")
    }

    override fun importSymbol(symbol: Symbol, alias: String?) {
        if (symbol.namespace.isNotEmpty() && symbol.namespace != namespace) {
            addImport(symbol.namespace, symbol.name, alias ?: symbol.name)
        }
    }
}

private data class UseStatement(val moduleName: String, val symbolName: String, val alias: String) {
    val rendered: String
        get() {
            val alias = alias.let { if (it == symbolName) "" else " as $it" }
            return "use $moduleName::$symbolName$alias;"
        }

    override fun toString(): String = rendered
}
