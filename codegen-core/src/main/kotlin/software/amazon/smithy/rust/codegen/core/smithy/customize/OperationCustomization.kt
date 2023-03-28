/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customize

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol

sealed class OperationSection(name: String) : Section(name) {
    abstract val customizations: List<OperationCustomization>

    /** Write custom code into the `impl` block of this operation */
    data class OperationImplBlock(override val customizations: List<OperationCustomization>) :
        OperationSection("OperationImplBlock")

    /** Write additional functions inside the Input's impl block */
    data class InputImpl(
        override val customizations: List<OperationCustomization>,
        val operationShape: OperationShape,
        val inputShape: StructureShape,
        val protocol: Protocol,
    ) : OperationSection("InputImpl")

    data class MutateInput(
        override val customizations: List<OperationCustomization>,
        val input: String,
        val config: String,
    ) : OperationSection("MutateInput")

    /** Write custom code into the block that builds an operation
     *
     * [request]: Name of the variable holding the `aws_smithy_http::Request`
     * [config]: Name of the variable holding the service config.
     *
     * */
    data class MutateRequest(
        override val customizations: List<OperationCustomization>,
        val request: String,
        val config: String,
    ) : OperationSection("Feature")

    data class FinalizeOperation(
        override val customizations: List<OperationCustomization>,
        val operation: String,
        val config: String,
    ) : OperationSection("Finalize")

    data class MutateOutput(
        override val customizations: List<OperationCustomization>,
        val operationShape: OperationShape,
        /** Name of the response headers map (for referring to it in Rust code) */
        val responseHeadersName: String,
    ) : OperationSection("MutateOutput")

    /**
     * Allows for adding additional properties to the `extras` field on the
     * `aws_smithy_types::error::ErrorMetadata`.
     */
    data class PopulateErrorMetadataExtras(
        override val customizations: List<OperationCustomization>,
        /** Name of the generic error builder (for referring to it in Rust code) */
        val builderName: String,
        /** Name of the response status (for referring to it in Rust code) */
        val responseStatusName: String,
        /** Name of the response headers map (for referring to it in Rust code) */
        val responseHeadersName: String,
    ) : OperationSection("PopulateErrorMetadataExtras")

    /**
     * Hook to add custom code right before the response is parsed.
     */
    data class BeforeParseResponse(
        override val customizations: List<OperationCustomization>,
        val responseName: String,
    ) : OperationSection("BeforeParseResponse")
}

abstract class OperationCustomization : NamedCustomization<OperationSection>() {
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
