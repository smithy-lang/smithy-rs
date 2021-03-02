/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.smithy.customize.Section

sealed class OperationSection(name: String) : Section(name) {
    /** Write custom code into the `impl` block of this operation */
    object ImplBlock : OperationSection("ImplBlock")

    /** Write custom code into the block that builds an operation
     *
     * [request]: Name of the variable holding the `smithy_http::Request`
     * [config]: Name of the variable holding the service config.
     *
     * */
    data class MutateRequest(val request: String, val config: String) : OperationSection("Feature")

    data class FinalizeOperation(val operation: String, val config: String) : OperationSection("Finalize")
}

abstract class OperationCustomization : NamedSectionGenerator<OperationSection>() {
    open fun retryType(): RuntimeType? = null
}
