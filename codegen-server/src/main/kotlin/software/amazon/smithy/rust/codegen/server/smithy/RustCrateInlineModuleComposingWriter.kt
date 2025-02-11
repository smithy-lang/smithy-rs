/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.ModuleDocProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.module
import java.util.concurrent.ConcurrentHashMap

typealias DocWriter = () -> Any
typealias InlineModuleCreator = (Symbol, Writable) -> Unit

/**
 * Initializes RustCrate -> InnerModule data structure.
 */
fun RustCrate.initializeInlineModuleWriter(debugMode: Boolean): InnerModule =
    crateToInlineModule
        .getOrPut(this) { InnerModule(moduleDocProvider, debugMode) }

/**
 * Returns the InnerModule for the given RustCrate
 */
fun RustCrate.getInlineModuleWriter(): InnerModule {
    return crateToInlineModule.getOrPut(this) { InnerModule(moduleDocProvider, false) }
}

/**
 * Returns a function that can be used to create an inline module writer.
 */
fun RustCrate.createInlineModuleCreator(): InlineModuleCreator {
    return { symbol: Symbol, writable: Writable ->
        this.getInlineModuleWriter().withInlineModuleHierarchyUsingCrate(this, symbol.module()) {
            writable()
        }
    }
}

/**
 * If the passed in `shape` is a synthetic extracted shape resulting from a constrained struct member,
 * the `Writable` is called using the structure's builder module. Otherwise, the `Writable` is called
 * using the given `module`.
 */
fun RustCrate.withModuleOrWithStructureBuilderModule(
    module: RustModule,
    shape: Shape,
    codegenContext: ServerCodegenContext,
    codeWritable: Writable,
) {
    // All structure constrained-member-shapes code is generated inside the structure builder's module.
    val parentAndInlineModuleInfo =
        shape.getParentAndInlineModuleForConstrainedMember(codegenContext.symbolProvider, codegenContext.settings.codegenConfig.publicConstrainedTypes)
    if (parentAndInlineModuleInfo == null) {
        this.withModule(module, codeWritable)
    } else {
        val (parent, inline) = parentAndInlineModuleInfo
        val inlineWriter = this.getInlineModuleWriter()

        inlineWriter.withInlineModuleHierarchyUsingCrate(this, parent) {
            inlineWriter.withInlineModuleHierarchy(this, inline) {
                codeWritable(this)
            }
        }
    }
}

/**
 * If the passed in `shape` is a synthetic extracted shape resulting from a constrained struct member,
 * the `Writable` is called using the structure's builder module. Otherwise, the `Writable` is called
 * using shape's `module`.
 */
fun RustCrate.useShapeWriterOrUseWithStructureBuilder(
    shape: Shape,
    codegenContext: ServerCodegenContext,
    docWriter: DocWriter? = null,
    writable: Writable,
) {
    // All structure constrained-member-shapes code is generated inside the structure builder's module.
    val parentAndInlineModuleInfo =
        shape.getParentAndInlineModuleForConstrainedMember(codegenContext.symbolProvider, codegenContext.settings.codegenConfig.publicConstrainedTypes)
    if (parentAndInlineModuleInfo == null) {
        docWriter?.invoke()
        this.useShapeWriter(shape, writable)
    } else {
        val (parent, inline) = parentAndInlineModuleInfo
        val inlineWriter = this.getInlineModuleWriter()

        inlineWriter.withInlineModuleHierarchyUsingCrate(this, parent) {
            inlineWriter.withInlineModuleHierarchy(this, inline) {
                writable(this)
            }
        }
    }
}

fun RustCrate.renderInlineMemoryModules() {
    val inlineModule = crateToInlineModule[this]
    check(inlineModule != null) {
        "InlineModule writer has not been registered for this crate"
    }
    inlineModule.render()
}

/**
 * Given a `RustWriter` calls the `Writable` using a `RustWriter` for the `inlineModule`
 */
fun RustCrate.withInMemoryInlineModule(
    outerWriter: RustWriter,
    inlineModule: RustModule.LeafModule,
    docWriter: DocWriter?,
    codeWritable: Writable,
) {
    check(inlineModule.isInline()) {
        "Module has to be an inline module for it to be used with the InlineModuleWriter"
    }
    this.getInlineModuleWriter().withInlineModuleHierarchy(outerWriter, inlineModule, docWriter) {
        codeWritable(this)
    }
}

fun RustWriter.createTestInlineModuleCreator(): InlineModuleCreator {
    return { symbol: Symbol, writable: Writable ->
        this.withInlineModule(symbol.module(), null) {
            writable()
        }
    }
}

/**
 * Maintains the `RustWriter` that has been created for a `RustModule.LeafModule`.
 */
private data class InlineModuleWithWriter(val inlineModule: RustModule.LeafModule, val writer: RustWriter)

/**
 * For each RustCrate a separate mapping of inline-module to `RustWriter` is maintained.
 */
private val crateToInlineModule: ConcurrentHashMap<RustCrate, InnerModule> =
    ConcurrentHashMap()

class InnerModule(private val moduleDocProvider: ModuleDocProvider, debugMode: Boolean) {
    // Holds the root modules to start rendering the descendents from.
    private val topLevelModuleWriters: ConcurrentHashMap<RustWriter, Unit> = ConcurrentHashMap()
    private val inlineModuleWriters: ConcurrentHashMap<RustWriter, MutableList<InlineModuleWithWriter>> =
        ConcurrentHashMap()
    private val docWriters: ConcurrentHashMap<RustModule.LeafModule, MutableList<DocWriter>> = ConcurrentHashMap()
    private val writerCreator = RustWriter.factory(debugMode)

    // By default, when a RustWriter is rendered, it prints a comment on top
    // indicating that it contains generated code and should not be manually edited. This comment
    // appears on each descendent inline module. To remove those comments, each time an inline
    // module is rendered, first `emptyLineCount` characters are removed from it.
    private val emptyLineCount: Int =
        writerCreator
            .apply("lines-it-always-writes.rs", "crate")
            .toString()
            .split("\n")[0]
            .length

    fun withInlineModule(
        outerWriter: RustWriter,
        innerModule: RustModule.LeafModule,
        docWriter: DocWriter? = null,
        writable: Writable,
    ) {
        if (docWriter != null) {
            val moduleDocWriterList = docWriters.getOrPut(innerModule) { mutableListOf() }
            moduleDocWriterList.add(docWriter)
        }
        writable(getWriter(outerWriter, innerModule))
    }

    /**
     * Given a `RustCrate` and a `RustModule.LeafModule()`, it creates a writer to that module and calls the writable.
     */
    fun withInlineModuleHierarchyUsingCrate(
        rustCrate: RustCrate,
        inlineModule: RustModule.LeafModule,
        docWriter: DocWriter? = null,
        writable: Writable,
    ) {
        val hierarchy = getHierarchy(inlineModule).toMutableList()
        check(!hierarchy.first().isInline()) {
            "When adding a `RustModule.LeafModule` to the crate, the topmost module in the hierarchy cannot be an inline module."
        }
        // The last in the hierarchy is the one we will return the writer for.
        val bottomMost = hierarchy.removeLast()

        // In case it is a top level module that has been passed (e.g. ModelsModule, OutputsModule) then
        // register it with the topLevel writers and call the writable on it. Otherwise, go over the
        // complete hierarchy, registering each of the inner modules and then call the `Writable`
        // with the bottom most inline module that has been passed.
        if (hierarchy.isNotEmpty()) {
            val topMost = hierarchy.removeFirst()

            // Create an intermediate writer for all inner modules in the hierarchy.
            rustCrate.withModule(topMost) {
                var writer = this
                hierarchy.forEach {
                    writer = getWriter(writer, it)
                }

                withInlineModule(writer, bottomMost, docWriter, writable)
            }
        } else {
            check(!bottomMost.isInline()) {
                "There is only one module in the hierarchy, so it has to be non-inlined."
            }
            rustCrate.withModule(bottomMost) {
                registerTopMostWriter(this)
                writable(this)
            }
        }
    }

    /**
     * Given a `Writer` to a module and an inline `RustModule.LeafModule()`, it creates a writer to that module and calls the writable.
     * It registers the complete hierarchy including the `outerWriter` if that is not already registrered.
     */
    fun withInlineModuleHierarchy(
        outerWriter: RustWriter,
        inlineModule: RustModule.LeafModule,
        docWriter: DocWriter? = null,
        writable: Writable,
    ) {
        val hierarchy = getHierarchy(inlineModule).toMutableList()
        if (!hierarchy.first().isInline()) {
            hierarchy.removeFirst()
        }
        check(hierarchy.isNotEmpty()) {
            "An inline module should always have one parent besides itself."
        }

        // The last in the hierarchy is the module under which the new inline module resides.
        val bottomMost = hierarchy.removeLast()

        // Create an entry in the HashMap for all the descendent modules in the hierarchy.
        var writer = outerWriter
        hierarchy.forEach {
            writer = getWriter(writer, it)
        }

        withInlineModule(writer, bottomMost, docWriter, writable)
    }

    /**
     * Creates an in memory writer and registers it with a map of RustWriter -> listOf(Inline descendent modules)
     */
    private fun createNewInlineModule(): RustWriter {
        val writer = writerCreator.apply("unknown-module-would-never-be-written.rs", "crate")
        // Register the new RustWriter in the map to allow further descendent inline modules to be created inside it.
        inlineModuleWriters[writer] = mutableListOf()
        return writer
    }

    /**
     * Returns the complete hierarchy of a `RustModule.LeafModule` from top to bottom
     */
    private fun getHierarchy(module: RustModule.LeafModule): List<RustModule.LeafModule> {
        var current: RustModule = module
        var hierarchy = listOf<RustModule.LeafModule>()

        while (current is RustModule.LeafModule) {
            hierarchy = listOf(current) + hierarchy
            current = current.parent
        }

        return hierarchy
    }

    /**
     * Writes out each inline module's code (`toString`) to the respective top level `RustWriter`.
     */
    fun render() {
        var writerToAddDependencies: RustWriter? = null

        fun writeInlineCode(
            rustWriter: RustWriter,
            code: String,
        ) {
            val inlineCode = code.drop(emptyLineCount)
            rustWriter.writeWithNoFormatting(inlineCode)
        }

        fun renderDescendents(
            topLevelWriter: RustWriter,
            inMemoryWriter: RustWriter,
        ) {
            // Traverse all descendent inline modules and render them.
            inlineModuleWriters[inMemoryWriter]!!.forEach {
                writeDocs(it.inlineModule)

                topLevelWriter.withInlineModule(it.inlineModule, moduleDocProvider) {
                    writeInlineCode(this, it.writer.toString())
                    renderDescendents(this, it.writer)
                }

                // Add dependencies introduced by the inline module to the top most RustWriter.
                it.writer.dependencies.forEach { dep -> writerToAddDependencies!!.addDependency(dep) }
            }
        }

        // Go over all the top level modules, create an `inlineModule` on the `RustWriter`
        // and call the descendent hierarchy renderer using the `inlineModule::RustWriter`.
        topLevelModuleWriters.keys.forEach {
            writerToAddDependencies = it

            check(inlineModuleWriters[it] != null) {
                "There must be a registered RustWriter for this module."
            }

            renderDescendents(it, it)
        }
    }

    /**
     * Given the inline-module returns an existing `RustWriter`, or if that inline module
     * has never been registered before then a new `RustWriter` is created and returned.
     */
    private fun getWriter(
        outerWriter: RustWriter,
        inlineModule: RustModule.LeafModule,
    ): RustWriter {
        val nestedModuleWriter = inlineModuleWriters[outerWriter]
        if (nestedModuleWriter != null) {
            return findOrAddToList(nestedModuleWriter, inlineModule)
        }

        val inlineWriters = registerTopMostWriter(outerWriter)
        return findOrAddToList(inlineWriters, inlineModule)
    }

    /**
     * Records the root of a dependency graph of inline modules.
     */
    private fun registerTopMostWriter(outerWriter: RustWriter): MutableList<InlineModuleWithWriter> {
        topLevelModuleWriters[outerWriter] = Unit
        return inlineModuleWriters.getOrPut(outerWriter) { mutableListOf() }
    }

    /**
     * Either gets a new `RustWriter` for the inline module or creates a new one and adds it to
     * the list of inline modules.
     */
    private fun findOrAddToList(
        inlineModuleList: MutableList<InlineModuleWithWriter>,
        lookForModule: RustModule.LeafModule,
    ): RustWriter {
        val index = inlineModuleList.indexOfFirst { it.inlineModule.name == lookForModule.name }

        return if (index == -1) {
            val inlineWriter = createNewInlineModule()
            inlineModuleList.add(InlineModuleWithWriter(lookForModule, inlineWriter))
            inlineWriter
        } else {
            val existing = inlineModuleList[index]

            // Verify everything except documentation matches.
            val existingModule = existing.inlineModule
            check(
                existingModule.name == lookForModule.name &&
                    existingModule.rustMetadata == lookForModule.rustMetadata &&
                    existingModule.parent == lookForModule.parent &&
                    existingModule.tests == lookForModule.tests,
            ) {
                """
                An inline module with the same name `${lookForModule.name}` was earlier created with different attributes:
                1) Metadata:
                    `${existingModule.rustMetadata}`
                    `${lookForModule.rustMetadata}`
                2) Parent:
                    `${existingModule.parent}`
                    `${lookForModule.parent}`
                3) Tests:
                    `${existingModule.tests}`
                    `${lookForModule.tests}`
                4) DocumentationOverride:
                    `${existingModule.documentationOverride}`
                    `${lookForModule.documentationOverride}`
                """
            }

            // Merge documentation
            val mergedDoc =
                when {
                    existingModule.documentationOverride == null -> lookForModule.documentationOverride
                    lookForModule.documentationOverride == null -> existingModule.documentationOverride
                    else -> "${existingModule.documentationOverride}\n${lookForModule.documentationOverride}"
                }

            // Replace the element in the list with merged documentation
            inlineModuleList[index] =
                InlineModuleWithWriter(
                    existingModule.copy(documentationOverride = mergedDoc),
                    existing.writer,
                )

            existing.writer
        }
    }

    private fun writeDocs(innerModule: RustModule.LeafModule) {
        docWriters[innerModule]?.forEach {
            it()
        }
    }
}
