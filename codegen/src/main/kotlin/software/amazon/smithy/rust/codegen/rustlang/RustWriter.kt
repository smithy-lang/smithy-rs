/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.rustlang

import org.intellij.lang.annotations.Language
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.writer.CodegenWriter
import software.amazon.smithy.codegen.core.writer.CodegenWriterFactory
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.utils.CodeWriter
import java.util.function.BiFunction

fun <T : CodeWriter> T.withBlock(
    textBeforeNewLine: String,
    textAfterNewLine: String,
    vararg args: Any,
    block: T.() -> Unit
): T {
    return conditionalBlock(textBeforeNewLine, textAfterNewLine, conditional = true, block = block, args = args)
}

/**
 * Write a block to the writer.
 * If [conditional] is true, the [textBeforeNewLine], followed by [block], followed by [textAfterNewLine]
 * If [conditional] is false, only [block] is written.
 * This enables conditionally wrapping a block in a prefix/suffix, eg.
 *
 * ```
 * writer.withBlock("Some(", ")", conditional = symbol.isOptional()) {
 *      write("symbolValue")
 * }
 * ```
 */
fun <T : CodeWriter> T.conditionalBlock(
    textBeforeNewLine: String,
    textAfterNewLine: String,
    conditional: Boolean = true,
    vararg args: Any,
    block: T.() -> Unit
): T {
    if (conditional) {
        openBlock(textBeforeNewLine, *args)
    }

    block(this)
    if (conditional) {
        closeBlock(textAfterNewLine)
    }
    return this
}

/**
 * Convenience wrapper that tells Intellij that the contents of this block are Rust
 */
fun <T : CodeWriter> T.rust(
    @Language("Rust", prefix = "macro_rules! foo { () =>  {{ ", suffix = "}}}") contents: String,
    vararg args: Any
) {
    this.write(contents, *args)
}

/**
 * Experimental API for templating long blocks of Rust
 *
 * This enables writing code like:
 *
 * ```kotlin
 * writer.rustTemplate("""
 * let some_val = #{operation}::from_response(response);
 * let serialized = #{serde_json}::to_json(some_val);
 * """, "operation" to operationSymbol, "serde_json" to RuntimeType.SerdeJson)
 * ```
 *
 * Before being passed to the underlying code writer, this syntax is rewritten to match the slightly more verbose
 * `CodeWriter` formatting: `#{name:T}`
 */
fun <T : CodeWriter> T.rustTemplate(
    @Language("Rust", prefix = "macro_rules! foo { () =>  {{ ", suffix = "}}}") contents: String,
    vararg ctx: Pair<String, Any>
) {
    this.pushState()
    this.putContext(ctx.toMap())
    this.write(contents.replace(Regex("""#\{([a-zA-Z_0-9]+)\}""")) { matchResult -> "#{${matchResult.groupValues[1]}:T}" })
    this.popState()
}

/*
 * Writes a Rust-style block, demarcated by curly braces
 */
fun <T : CodeWriter> T.rustBlock(
    @Language("Rust", prefix = "macro_rules! foo { () =>  {{ ", suffix = "}}}")
    header: String,
    vararg args: Any,
    block: T.() -> Unit
): T {
    openBlock("$header {", *args)
    block(this)
    closeBlock("}")
    return this
}

/**
 * Generate a RustDoc comment for [shape]
 */
fun <T : CodeWriter> T.documentShape(shape: Shape, model: Model): T {
    // TODO: support additional Smithy documentation traits like @example
    val docTrait = shape.getMemberTrait(model, DocumentationTrait::class.java).orNull()

    docTrait?.value?.also {
        this.docs(escape(it))
    }

    return this
}

/**
 * Write RustDoc-style docs into the writer
 *
 * Several modifications are made to provide consistent RustDoc formatting:
 *    - All lines will be prefixed by `///`
 *    - Tabs are replaced with spaces
 *    - Empty newlines are removed
 */
fun <T : CodeWriter> T.docs(text: String, vararg args: Any, newlinePrefix: String = "/// "): T {
    pushState()
    setNewlinePrefix(newlinePrefix)
    // TODO: Smithy updates should remove the need for a number of these changes
    val cleaned = text.lines()
        // We need to filter out blank lines—an empty line causes the markdown parser to interpret the subsequent
        // docs as a code block because they are indented.
        .filter { it.isNotBlank() }
        .joinToString("\n") {
            // Rustdoc warns on tabs in documentation
            it.trimStart().replace("\t", "  ")
        }
    write(cleaned, *args)
    popState()
    return this
}

/** Escape the [expressionStart] character to avoid problems during formatting */
fun CodeWriter.escape(text: String): String = text.replace("$expressionStart", "$expressionStart$expressionStart")

/**
 * Write _exactly_ the text as written into the code writer without newlines or formatting
 */
fun CodeWriter.raw(text: String) = writeInline(escape(text))

typealias Writable = RustWriter.() -> Unit

/** Helper to allow coercing the Writeable signature
 *  writable { rust("fn foo() { }")
 */
fun writable(w: Writable): Writable = w

fun writable(w: String): Writable = writable { rust(w) }

class RustWriter private constructor(
    private val filename: String,
    val namespace: String,
    private val commentCharacter: String = "//",
    private val printWarning: Boolean = true
) :
    CodegenWriter<RustWriter, UseDeclarations>(null, UseDeclarations(namespace)) {
    private var headerDocs: String? = null
    fun setHeaderDocs(docs: String) {
        headerDocs = RustWriter("", "", printWarning = false).docs(docs, newlinePrefix = "//! ").toString()
    }

    companion object {
        fun root() = forModule(null)
        fun forModule(module: String?): RustWriter = if (module == null) {
            RustWriter("lib.rs", "crate")
        } else {
            RustWriter("$module.rs", "crate::$module")
        }

        val Factory: CodegenWriterFactory<RustWriter> =
            CodegenWriterFactory<RustWriter> { filename, namespace ->
                when {
                    filename.endsWith(".toml") -> RustWriter(filename, namespace, "#")
                    else -> RustWriter(filename, namespace)
                }
            }
    }

    private val formatter = RustSymbolFormatter()
    private var n = 0

    init {
        expressionStart = '#'
        if (filename.endsWith(".rs")) {
            require(namespace.startsWith("crate")) { "We can only write into files in the crate (got $namespace)" }
        }
        putFormatter('T', formatter)
        putFormatter('D', RustDocLinker())
    }

    fun module(): String? = if (filename.endsWith(".rs")) {
        filename.removeSuffix(".rs").split('/').last()
    } else null

    fun safeName(prefix: String = "var"): String {
        n += 1
        return "${prefix}_$n"
    }

    /**
     * Create an inline module.
     *
     * Callers must take care to use [this] when writing to ensure code is written to the right place:
     * ```kotlin
     * val writer = RustWriter.forModule("model")
     * writer.withModule("nested") {
     *   Generator(...).render(this) // GOOD
     *   Generator(...).render(writer) // WRONG!
     * }
     * ```
     *
     * The returned writer will inject any local imports into the module as needed.
     */
    fun withModule(
        moduleName: String,
        rustMetadata: RustMetadata = RustMetadata(public = true),
        moduleWriter: RustWriter.() -> Unit
    ): RustWriter {
        // In Rust, modules must specify their own imports—they don't have access to the parent scope.
        // To easily handle this, create a new inner writer to collect imports, then dump it
        // into an inline module.
        val innerWriter = RustWriter(this.filename, "${this.namespace}::$moduleName", printWarning = false)
        moduleWriter(innerWriter)
        rustMetadata.render(this)
        rustBlock("mod $moduleName") {
            writeWithNoFormatting(innerWriter.toString())
        }
        innerWriter.dependencies.forEach { addDependency(it) }
        return this
    }

    /**
     * Generate a wrapping if statement around a field.
     *
     * - If the field is optional, it will only be called if the field is present
     * - If the field is an unboxed primitive, it will only be called if the field is non-zero
     *
     */
    fun ifSet(shape: Shape, member: Symbol, outerField: String, block: CodeWriter.(field: String) -> Unit) {
        // TODO: this API should be refactored so that we don't need to strip `&` to get reference comparisons to work.
        when {
            member.isOptional() -> {
                val derefName = safeName("inner")
                rustBlock("if let Some($derefName) = $outerField") {
                    block(derefName)
                }
            }
            shape is NumberShape -> rustBlock("if ${outerField.removePrefix("&")} != 0") {
                block(outerField)
            }
            shape is BooleanShape -> rustBlock("if ${outerField.removePrefix("&")}") {
                block(outerField)
            }
            else -> this.block(outerField)
        }
    }

    fun ListForEach(
        target: Shape,
        outerField: String,
        block: CodeWriter.(field: String, target: ShapeId) -> Unit
    ) {
        if (target is CollectionShape) {
            val derefName = safeName("inner")
            rustBlock("for $derefName in $outerField") {
                block(derefName, target.member.target)
            }
        } else {
            this.block(outerField, target.toShapeId())
        }
    }

    override fun toString(): String {
        val contents = super.toString()
        // Hack to support TOML
        // TODO: consider creating a TOML writer
        val header = if (printWarning) {
            "$commentCharacter Code generated by software.amazon.smithy.rust.codegen.smithy-rs. DO NOT EDIT."
        } else ""
        val useDecls = importContainer.toString()
        return "${headerDocs ?: ""}\n$header\n$useDecls\n$contents\n"
    }

    fun format(r: Any):
        String {
            return formatter.apply(r, "")
        }

    fun useAs(target: Shape, base: String): String {
        return if (target.hasTrait(EnumTrait::class.java)) {
            "$base.as_str()"
        } else {
            base
        }
    }

    fun addDepsRecursively(symbol: Symbol) {
        addDependency(symbol)
        symbol.references.forEach { addDepsRecursively(it.symbol) }
    }

    /**
     * Generate RustDoc links, eg. [`Abc`](crate::module::Abc)
     */
    inner class RustDocLinker : BiFunction<Any, String, String> {
        override fun apply(t: Any, u: String): String {
            return when (t) {
                is Symbol -> "[`${t.name}`](${t.fullName})"
                else -> throw CodegenException("Invalid type provided to RustDocLinker ($t) expected Symbol")
            }
        }
    }

    inner class RustSymbolFormatter : BiFunction<Any, String, String> {
        override fun apply(t: Any, u: String): String {
            return when (t) {
                is RuntimeType -> {
                    t.dependency?.also { addDependency(it) }
                    // for now, use the fully qualified type name
                    t.fullyQualifiedName()
                }
                is Symbol -> {
                    addDepsRecursively(t)
                    t.rustType().render(fullyQualified = true)
                }
                else -> throw CodegenException("Invalid type provided to RustSymbolFormatter")
            }
        }
    }
}
