/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

// This decorator lives in codegen-client (not aws/codegen-aws-sdk) because
// `aws.api#longPoll` is a Smithy trait that applies to any Smithy service,
// not just AWS services. The hard-coded operation list is used until the
// trait is available in Smithy models.
//
// TODO(Post Retry2.1): Replace this hard-coded list with a check for the
//  `aws.api#longPoll` Smithy trait once it is available.
private val LONG_POLLING_OPERATIONS =
    setOf(
        "com.amazonaws.sqs#ReceiveMessage",
        "com.amazonaws.sfn#GetActivityTask",
        "com.amazonaws.swf#PollForActivityTask",
        "com.amazonaws.swf#PollForDecisionTask",
    )

class LongPollingOperationDecorator : ClientCodegenDecorator {
    override val name: String = "LongPollingOperation"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        if (operation.id.toString() in LONG_POLLING_OPERATIONS) {
            baseCustomizations + LongPollingOperationCustomization(codegenContext)
        } else {
            baseCustomizations
        }
}

private class LongPollingOperationCustomization(
    private val codegenContext: ClientCodegenContext,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            if (section is OperationSection.AdditionalInterceptors) {
                val rc = codegenContext.runtimeConfig
                val longPollingInterceptor =
                    RuntimeType.forInlineDependency(
                        InlineDependency.forRustFile(
                            RustModule.pubCrate("long_polling", parent = ClientRustModule.root),
                            "/inlineable/src/long_polling.rs",
                            CargoDependency.smithyRuntimeApiClient(rc),
                            CargoDependency.smithyTypes(rc),
                        ),
                    ).resolve("LongPollingInterceptor")

                section.registerPermanentInterceptor(rc, this) {
                    rustTemplate(
                        "#{LongPollingInterceptor}",
                        "LongPollingInterceptor" to longPollingInterceptor,
                    )
                }
            }
        }
}
