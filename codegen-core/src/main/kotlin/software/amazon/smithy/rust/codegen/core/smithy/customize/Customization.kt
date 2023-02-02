/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customize

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable

/**
 * An overrideable section for code generation. Usage:
 * ```kotlin
 * sealed class OperationCustomization(name: String) : Section(name) {
 *      // Sections can be state-carrying to allow implementations to make different choices based on
 *      // different operations
 *      data class RequestCreation(protocolConfig: ProtocolConfig, operation: OperationShape) : Section("RequestCreation")
 *      // Sections can be stateless, e.g. this section that could write code into the
 *      // top level operation module
 *      object OperationModule : ServiceConfig("OperationTopLevel")
 * }
 */
abstract class Section(val name: String)

/**
 * Detached section abstraction to allow adhoc sections to be created. By using the `.writer` method, an
 * instantiation of this section can be easily created.
 */
abstract class AdHocCustomization : NamedCustomization<Section>() {
    companion object {
        /**
         * Helper to enable easily combining detached sections with the [CoreCodegenDecorator.extraSections] method.
         *
         * Note: When calling this, be sure to set the type of the section. For example:
         * ```
         * // The part in angle brackets is important:
         * AdHocCustomization.customize<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
         *     // snip
         * },
         * ```
         */
        inline fun <reified T> customize(
            crossinline customization: RustWriter.(T) -> Unit,
        ): AdHocCustomization =
            object : AdHocCustomization() {
                override fun section(section: Section): Writable = writable {
                    if (section is T) {
                        customization(section)
                    }
                }
            }
    }
}

/**
 * A named section generator allows customization via a predefined set of named sections.
 *
 * Implementors MUST override section and use a `when` clause to handle each section individually
 */
abstract class NamedCustomization<T : Section> {
    abstract fun section(section: T): Writable
    protected val emptySection = writable { }
}

/** Convenience for rendering a list of customizations for a given section */
fun <T : Section> RustWriter.writeCustomizations(customizations: List<NamedCustomization<T>>, section: T) {
    for (customization in customizations) {
        customization.section(section)(this)
    }
}
