/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.RustType

/**
 * A hierarchy of Python types handled by Smithy codegen.
 *
 * Mostly copied from [RustType] and modified for Python accordingly.
 */
sealed class PythonType {
    /**
     * A Python type that contains [member], another [PythonType].
     * Used to generically operate over shapes that contain other shape.
     */
    sealed interface Container {
        val member: PythonType
        val namespace: String?
        val name: String
    }

    /**
     * Name refers to the top-level type for import purposes.
     */
    abstract val name: String

    open val namespace: String? = null

    object None : PythonType() {
        override val name: String = "None"
    }

    object Bool : PythonType() {
        override val name: String = "bool"
    }

    object Int : PythonType() {
        override val name: String = "int"
    }

    object Float : PythonType() {
        override val name: String = "float"
    }

    object Str : PythonType() {
        override val name: String = "str"
    }

    object Any : PythonType() {
        override val name: String = "Any"
        override val namespace: String = "typing"
    }

    data class List(override val member: PythonType) : PythonType(), Container {
        override val name: String = "List"
        override val namespace: String = "typing"
    }

    data class Dict(val key: PythonType, override val member: PythonType) : PythonType(), Container {
        override val name: String = "Dict"
        override val namespace: String = "typing"
    }

    data class Set(override val member: PythonType) : PythonType(), Container {
        override val name: String = "Set"
        override val namespace: String = "typing"
    }

    data class Optional(override val member: PythonType) : PythonType(), Container {
        override val name: String = "Optional"
        override val namespace: String = "typing"
    }

    data class Awaitable(override val member: PythonType) : PythonType(), Container {
        override val name: String = "Awaitable"
        override val namespace: String = "typing"
    }

    data class Callable(val args: kotlin.collections.List<PythonType>, val rtype: PythonType) : PythonType() {
        override val name: String = "Callable"
        override val namespace: String = "typing"
    }

    data class Union(val args: kotlin.collections.List<PythonType>) : PythonType() {
        override val name: String = "Union"
        override val namespace: String = "typing"
    }

    data class AsyncIterator(override val member: PythonType) : PythonType(), Container {
        override val name: String = "AsyncIterator"
        override val namespace: String = "typing"
    }

    data class Application(val type: PythonType, val args: kotlin.collections.List<PythonType>) : PythonType() {
        override val name = type.name
        override val namespace = type.namespace
    }

    data class Opaque(override val name: String, val pythonRootModuleName: String, val rustNamespace: String? = null) : PythonType() {
        override val namespace: String? =
            rustNamespace?.split("::")?.joinToString(".") {
                when (it) {
                    "crate" -> pythonRootModuleName
                    // In Python, we expose submodules from `aws_smithy_http_server_python`
                    // like `types`, `middleware`, `tls` etc. from Python root module
                    "aws_smithy_http_server_python" -> pythonRootModuleName
                    else -> it
                }
            }
                // Most opaque types have a leading `::`, so strip that for Python as needed
                .let {
                    when (it?.startsWith(".")) {
                        true -> it.substring(1)
                        else -> it
                    }
                }
    }
}

/**
 * Return corresponding [PythonType] for a [RustType].
 */
fun RustType.pythonType(pythonRootModuleName: String): PythonType =
    when (this) {
        is RustType.Unit -> PythonType.None
        is RustType.Bool -> PythonType.Bool
        is RustType.Float -> PythonType.Float
        is RustType.Integer -> PythonType.Int
        is RustType.String -> PythonType.Str
        is RustType.Vec -> PythonType.List(this.member.pythonType(pythonRootModuleName))
        is RustType.Slice -> PythonType.List(this.member.pythonType(pythonRootModuleName))
        is RustType.HashMap -> PythonType.Dict(this.key.pythonType(pythonRootModuleName), this.member.pythonType(pythonRootModuleName))
        is RustType.HashSet -> PythonType.Set(this.member.pythonType(pythonRootModuleName))
        is RustType.Reference -> this.member.pythonType(pythonRootModuleName)
        is RustType.Option -> PythonType.Optional(this.member.pythonType(pythonRootModuleName))
        is RustType.Box -> this.member.pythonType(pythonRootModuleName)
        is RustType.Dyn -> this.member.pythonType(pythonRootModuleName)
        is RustType.Application ->
            PythonType.Application(
                this.type.pythonType(pythonRootModuleName),
                this.args.map {
                    it.pythonType(pythonRootModuleName)
                },
            )
        is RustType.Opaque -> PythonType.Opaque(this.name, pythonRootModuleName, rustNamespace = this.namespace)
        is RustType.MaybeConstrained -> this.member.pythonType(pythonRootModuleName)
    }

/**
 * Render this type, including references and generic parameters.
 * It generates something like `typing.Dict[String, String]`.
 */
fun PythonType.render(fullyQualified: Boolean = true): String {
    val namespace =
        if (fullyQualified) {
            this.namespace?.let { "$it." } ?: ""
        } else {
            ""
        }
    val base =
        when (this) {
            is PythonType.None -> this.name
            is PythonType.Bool -> this.name
            is PythonType.Float -> this.name
            is PythonType.Int -> this.name
            is PythonType.Str -> this.name
            is PythonType.Any -> this.name
            is PythonType.Opaque -> this.name
            is PythonType.List -> "${this.name}[${this.member.render(fullyQualified)}]"
            is PythonType.Dict -> "${this.name}[${this.key.render(fullyQualified)}, ${this.member.render(fullyQualified)}]"
            is PythonType.Set -> "${this.name}[${this.member.render(fullyQualified)}]"
            is PythonType.Awaitable -> "${this.name}[${this.member.render(fullyQualified)}]"
            is PythonType.Optional -> "${this.name}[${this.member.render(fullyQualified)}]"
            is PythonType.AsyncIterator -> "${this.name}[${this.member.render(fullyQualified)}]"
            is PythonType.Application -> {
                val args = this.args.joinToString(", ") { it.render(fullyQualified) }
                "${this.name}[$args]"
            }
            is PythonType.Callable -> {
                val args = this.args.joinToString(", ") { it.render(fullyQualified) }
                val rtype = this.rtype.render(fullyQualified)
                "${this.name}[[$args], $rtype]"
            }
            is PythonType.Union -> {
                val args = this.args.joinToString(", ") { it.render(fullyQualified) }
                "${this.name}[$args]"
            }
        }
    return "$namespace$base"
}

/**
 * Renders [PythonType] with proper escaping for Docstrings.
 */
fun PythonType.renderAsDocstring(): String =
    this.render()
        .replace("[", "\\[")
        .replace("]", "\\]")
