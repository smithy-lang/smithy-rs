/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import software.amazon.smithy.rust.codegen.smithy.RuntimeType

sealed class OperationSection(name: String) : Section(name) {
    /** Write custom code into the `impl` block of this operation */
    object OperationImplBlock : OperationSection("OperationImplBlock")

    data class MutateInput(val input: String, val config: String) : OperationSection("MutateInput")

    /** Write custom code into the block that builds an operation
     *
     * [request]: Name of the variable holding the `smithy_http::Request`
     * [config]: Name of the variable holding the service config.
     *
     * */
    data class MutateRequest(val request: String, val config: String) : OperationSection("Feature")

    data class FinalizeOperation(val operation: String, val config: String) : OperationSection("Finalize")

    /**
     * Update the generic error for a protocol
     *
     * The resulting code must create a binding that shadows the original `genericError`
     */
    data class UpdateGenericError(val genericError: String, val httpResponse: String) : OperationSection("UpdateGenericError")
}

abstract class OperationCustomization : NamedSectionGenerator<OperationSection>() {
    open fun retryType(): RuntimeType? = null

    /**
     * Does `make_operation` consume the self parameter?
     *
     * This is required for things like idempotency tokens where the operation can only be sent once
     * and an idempotency token will mutate the request.
     */
    open fun consumesSelf(): Boolean = false

    /**
     * Does `make_operation` mutate the self parameter?
     */
    open fun mutSelf(): Boolean = false
}
