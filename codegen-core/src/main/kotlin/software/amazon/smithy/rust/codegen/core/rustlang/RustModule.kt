/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.PANIC

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
        val parent: RustModule = LibRs,
        val inline: Boolean = false,
        // module is a cfg(test) module
        val tests: Boolean = false,
        val documentationOverride: String? = null,
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

        /** Convert a module into a module gated with `#[cfg(test)]` */
        fun cfgTest(): LeafModule =
            this.copy(
                rustMetadata = rustMetadata.copy(additionalAttributes = rustMetadata.additionalAttributes + Attribute.CfgTest),
                tests = true,
            )
    }

    companion object {
        /** Creates a new module with the specified visibility */
        fun new(
            name: String,
            visibility: Visibility,
            inline: Boolean = false,
            parent: RustModule = LibRs,
            additionalAttributes: List<Attribute> = listOf(),
            documentationOverride: String? = null,
        ): LeafModule {
            return LeafModule(
                RustReservedWords.escapeIfNeeded(name),
                RustMetadata(visibility = visibility, additionalAttributes = additionalAttributes),
                inline = inline,
                parent = parent,
                documentationOverride = documentationOverride,
            )
        }

        /** Creates a new public module */
        fun public(
            name: String,
            parent: RustModule = LibRs,
            documentationOverride: String? = null,
            additionalAttributes: List<Attribute> = emptyList(),
            inline: Boolean = false,
        ): LeafModule =
            new(
                name,
                visibility = Visibility.PUBLIC,
                inline = inline,
                parent = parent,
                documentationOverride = documentationOverride,
                additionalAttributes = additionalAttributes,
            )

        /** Creates a new private module */
        fun private(
            name: String,
            parent: RustModule = LibRs,
        ): LeafModule = new(name, visibility = Visibility.PRIVATE, inline = false, parent = parent)

        fun pubCrate(
            name: String,
            parent: RustModule = LibRs,
            additionalAttributes: List<Attribute> = emptyList(),
        ): LeafModule =
            new(
                name, visibility = Visibility.PUBCRATE,
                inline = false,
                parent = parent,
                additionalAttributes = additionalAttributes,
            )

        fun inlineTests(
            name: String = "test",
            parent: RustModule = LibRs,
            additionalAttributes: List<Attribute> = listOf(),
        ) = new(
            name,
            Visibility.PRIVATE,
            inline = true,
            additionalAttributes = additionalAttributes,
            parent = parent,
        ).cfgTest()
    }

    fun isInline(): Boolean =
        when (this) {
            is LibRs -> false
            is LeafModule -> this.inline
        }

    /**
     * Fully qualified path to this module, e.g. `crate::grandparent::parent::child`
     */
    fun fullyQualifiedPath(): String =
        when (this) {
            is LibRs -> "crate"
            is LeafModule -> parent.fullyQualifiedPath() + "::" + name
        }

    /**
     * The file this module is homed in, e.g. `src/grandparent/parent/child.rs`
     */
    fun definitionFile(): String =
        when (this) {
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
    fun renderModStatement(
        writer: RustWriter,
        moduleDocProvider: ModuleDocProvider,
    ) {
        when (this) {
            is LeafModule -> {
                if (name.startsWith("r#")) {
                    PANIC("Something went wrong with module name escaping (module named '$name'). This is a bug.")
                }
                ModuleDocProvider.writeDocs(moduleDocProvider, this, writer)
                rustMetadata.render(writer)
                writer.write("mod $name;")
            }

            else -> {}
        }
    }

    /** Converts this [RustModule] into a [RuntimeType] */
    fun toType(): RuntimeType = RuntimeType(fullyQualifiedPath())
}
