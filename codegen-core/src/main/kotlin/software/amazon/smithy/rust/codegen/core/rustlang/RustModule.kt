/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * RustModule system.
 *
 * RustModules are idempotent, BUT, you must take care to always use the same module (matching docs, visibility, etc.):
 * - There is no guarantee _which_ module will be rendered.
 */
sealed class RustModule {

    /** lib.rs */
    object LibRs : RustModule()

    /**
     * LeafModule
     *
     * A LeafModule is _all_ modules that are not `lib.rs`. To create a nested leaf module, set `parent` to a module
     * _other_ than `LibRs`.
     *
     * To avoid infinite loops, avoid setting parent to itself ;-)
     */
    data class LeafModule(
        val name: String,
        val rustMetadata: RustMetadata,
        val documentation: String? = null,
        val parent: RustModule = LibRs,
        val inline: Boolean = false,
    ) : RustModule() {
        init {
            check(!name.contains("::")) {
                "Module names CANNOT contain `::`—modules must be nested with parent (name was: `$name`)"
            }
            check(name != "") {
                "Module name cannot be empty"
            }

            check(!RustReservedWords.isReserved(name)) {
                "Module `$name` cannot be a module name—it is a reserved word."
            }
        }
    }

    companion object {

        /** Creates a new module with the specified visibility */
        fun new(
            name: String,
            visibility: Visibility,
            documentation: String? = null,
            inline: Boolean = false,
            parent: RustModule = LibRs,
            additionalAttributes: List<Attribute> = listOf(),
        ): LeafModule {
            return LeafModule(
                RustReservedWords.escapeIfNeeded(name),
                RustMetadata(visibility = visibility, additionalAttributes = additionalAttributes),
                documentation,
                inline = inline,
                parent = parent,
            )
        }

        /** Creates a new public module */
        fun public(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            new(name, visibility = Visibility.PUBLIC, documentation = documentation, inline = false, parent = parent)

        /** Creates a new private module */
        fun private(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            new(name, visibility = Visibility.PRIVATE, documentation = documentation, inline = false, parent = parent)

        fun pubCrate(name: String, documentation: String? = null, parent: RustModule): LeafModule =
            new(name, visibility = Visibility.PUBCRATE, documentation = documentation, inline = false, parent = parent)

        /* Common modules used across client, server and tests */
        val Config = public("config", documentation = "Configuration for the service.")
        val Error = public("error", documentation = "All error types that operations can return. Documentation on these types is copied from the model.")
        val Model = public("model", documentation = "Data structures used by operation inputs/outputs. Documentation on these types is copied from the model.")
        val Input = public("input", documentation = "Input structures for operations. Documentation on these types is copied from the model.")
        val Output = public("output", documentation = "Output structures for operations. Documentation on these types is copied from the model.")
        val Types = public("types", documentation = "Data primitives referenced by other data types.")

        /**
         * Helper method to generate the `operation` Rust module.
         * Its visibility depends on the generation context (client or server).
         */
        fun operation(visibility: Visibility): RustModule =
            new(
                "operation",
                visibility = visibility,
                documentation = "All operations that this crate can perform.",
            )
    }

    fun isInline(): Boolean = when (this) {
        is LibRs -> false
        is LeafModule -> this.inline
    }

    /**
     * Fully qualified path to this module, e.g. `crate::grandparent::parent::child`
     */
    fun fullyQualifiedPath(): String = when (this) {
        is LibRs -> "crate"
        is LeafModule -> parent.fullyQualifiedPath() + "::" + name
    }

    /**
     * The file this module is homed in, e.g. `src/grandparent/parent/child.rs`
     */
    fun definitionFile(): String = when (this) {
        is LibRs -> "src/lib.rs"
        is LeafModule -> {
            val path = fullyQualifiedPath().split("::").drop(1).joinToString("/")
            "src/$path.rs"
        }
    }

    /**
     * Renders the usage statement, approximately:
     * ```rust
     * /// My docs
     * pub mod my_module_name
     * ```
     */
    fun renderModStatement(writer: RustWriter) {
        when (this) {
            is LeafModule -> {
                documentation?.let { docs -> writer.docs(docs) }
                rustMetadata.render(writer)
                writer.write("mod $name;")
            }

            else -> {}
        }
    }

    /** Converts this [RustModule] into a [RuntimeType] */
    fun toType(): RuntimeType = RuntimeType(fullyQualifiedPath())
}
