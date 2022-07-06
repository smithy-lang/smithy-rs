/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customizations.AllowLintsGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.CrateVersionGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.EndpointPrefixGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.HttpChecksumRequiredGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.HttpVersionListCustomization
import software.amazon.smithy.rust.codegen.smithy.customizations.IdempotencyTokenGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.SmithyTypesPubUseGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization

/**
 * A set of customizations that are included in all protocols.
 *
 * This exists as a convenient place to gather these modifications, these are not true customizations.
 */
class RequiredCustomizations : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "Required"
    override val order: Byte = -1

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> =
        baseCustomizations +
            IdempotencyTokenGenerator(codegenContext, operation) +
            EndpointPrefixGenerator(codegenContext, operation) +
            HttpChecksumRequiredGenerator(codegenContext, operation) +
            HttpVersionListCustomization(codegenContext, operation)

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> =
        baseCustomizations + CrateVersionGenerator() + SmithyTypesPubUseGenerator(codegenContext.runtimeConfig) + AllowLintsGenerator()

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        // Add rt-tokio feature for `ByteStream::from_path`
        rustCrate.mergeFeature(Feature("rt-tokio", true, listOf("aws-smithy-http/rt-tokio")))
    }
}
