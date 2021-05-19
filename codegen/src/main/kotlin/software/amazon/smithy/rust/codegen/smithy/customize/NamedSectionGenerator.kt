/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.writable

/**
 * An overrideable section for code generation. Usage:
 * ```kotlin
 * sealed class OperationCustomization(name: String) : Section(name) {
 *      // Sections can be state-carrying to allow implementations to make different choices based on
 *      // different operations
 *      data class RequestCreation(protocolConfig: ProtocolConfig, operation: OperationShape) : Section("RequestCreation")
 *      // Sections can be stateless, eg. this section that could write code into the
 *      // top level operation module
 *      object OperationModule : ServiceConfig("OperationTopLevel")
 * }
 */
abstract class Section(val name: String)

/**
 * A named section generator allows customization via a predefined set of named sections. Implementors may either:
 * 1. Override section and use a `when` clause to handle each section individually
 * 2. Call `registerSection { ... }` to register individual sections on demand.
 *
 * ```rust
 * struct Config {
 *    /* section:ConfigStruct */
 *    make_token: TokenProvider
 *    /* endsection:ConfigStruct */
 * }
 * ```
 *
 * In cases where the generated code is static, this will improve readability.
 */
abstract class NamedSectionGenerator<T : Section> {
    abstract fun section(section: T): Writable
    protected val emptySection = writable { }
}
