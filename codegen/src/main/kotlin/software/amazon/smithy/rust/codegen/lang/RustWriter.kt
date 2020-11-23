/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.lang

import org.intellij.lang.annotations.Language
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.writer.CodegenWriter
import software.amazon.smithy.codegen.core.writer.CodegenWriterFactory
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.utils.CodeWriter
import java.util.Optional
import java.util.function.BiFunction

fun <T : CodeWriter> T.withBlock(
    textBeforeNewLine: String,
    textAfterNewLine: String,
    conditional: Boolean = true,
    block: T.() -> Unit
): T {
    if (conditional) {
        openBlock(textBeforeNewLine)
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
fun <T : CodeWriter> T.rust(@Language("Rust", prefix = "fn foo() {", suffix = "}") contents: String, vararg args: Any) {
    this.write(contents, *args)
}

/*
 * Writes a Rust-style block, demarcated by curly braces
 */
fun <T : CodeWriter> T.rustBlock(header: String, vararg args: Any, block: T.() -> Unit): T {
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
    val docTrait = shape.getTrait(DocumentationTrait::class.java).or {
        if (shape is MemberShape) {
            model.expectShape(shape.target).getTrait(DocumentationTrait::class.java)
        } else {
            Optional.empty()
        }
    }.orNull()

    docTrait?.value?.also {
        this.docs(it)
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
fun <T : CodeWriter> T.docs(text: String, vararg args: Any) {
    pushState("docs")
    setNewlinePrefix("/// ")
    val cleaned = text.lines()
        // We need to filter out blank lines—an empty line causes the markdown parser to interpret the subsequent
        // docs as a code block because they are indented.
        .filter { !it.isBlank() }
        // Rustdoc warns on tabs in documentation
        .map { it.trimStart().replace("\t", "  ") }
        .joinToString("\n")
    write(cleaned, *args)
    popState()
}

class RustWriter private constructor(
    private val filename: String,
    val namespace: String,
    private val commentCharacter: String = "//"
) :
    CodegenWriter<RustWriter, UseDeclarations>(null, UseDeclarations(namespace)) {
    companion object {
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

    init {
        if (filename.endsWith(".rs")) {
            require(namespace.startsWith("crate")) { "We can only write into files in the crate (got $namespace)" }
        }
    }

    private val formatter = RustSymbolFormatter()
    private var n = 0

    init {
        putFormatter('T', formatter)
        putFormatter('D', RustDocLinker())
    }

    fun module(): String? = if (filename.endsWith(".rs")) {
        filename.removeSuffix(".rs").split('/').last()
    } else null

    private fun safeName(prefix: String = "var"): String {
        n += 1
        return "${prefix}_$n"
    }

    /**
     * Create an inline module.
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
        val innerWriter = RustWriter(this.filename, "${this.namespace}::$moduleName")
        moduleWriter(innerWriter)
        rustMetadata.render(this)
        rustBlock("mod $moduleName") {
            write(innerWriter.toString())
        }
        innerWriter.dependencies.forEach { addDependency(it) }
        return this
    }

    // TODO: refactor both of these methods & add a parent method to for_each across any field type
    // generically
    fun OptionForEach(member: Symbol, outerField: String, block: CodeWriter.(field: String) -> Unit) {
        if (member.isOptional()) {
            val derefName = safeName("inner")
            // TODO: `inner` should be custom codegenned to avoid shadowing
            rustBlock("if let Some($derefName) = $outerField") {
                block(derefName)
            }
        } else {
            this.block(outerField)
        }
    }

    fun ListForEach(target: Shape, outerField: String, block: CodeWriter.(field: String, target: ShapeId) -> Unit) {
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
        val header = "$commentCharacter Code generated by software.amazon.smithy.rust.codegen.smithy-rs. DO NOT EDIT."
        val useDecls = importContainer.toString()
        return "$header\n$useDecls\n$contents\n"
    }

    fun format(r: Any): String {
        return formatter.apply(r, "")
    }

    fun useAs(target: Shape, base: String): String {
        return if (target.hasTrait(EnumTrait::class.java)) {
            "$base.as_str()"
        } else {
            base
        }
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
                    if (t.namespace != namespace) {
                        addImport(t, null)
                    }
                    t.rustType().render()
                }
                else -> throw CodegenException("Invalid type provided to RustSymbolFormatter")
            }
        }
    }
}
