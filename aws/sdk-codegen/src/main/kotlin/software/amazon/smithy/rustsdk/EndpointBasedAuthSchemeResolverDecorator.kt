/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.core.util.sdkId

/** Contains a list of SDK IDs that are allowed to use EndpointBasedAuthSchemeResolver */
private val EndpointBasedAuthSchemeAllowList =
    setOf(
        "cloudfrontkeyvaluestore",
        "eventbridge",
        "s3",
        "sesv2",
    )

class EndpointBasedAuthSchemeResolverDecorator : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.let {
            val sdkId = codegenContext.serviceShape.sdkId()
            EndpointBasedAuthSchemeAllowList.contains(sdkId)
        } ?: false
    },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "EndpointBasedAuthSchemeResolverDecorator"
            override val order: Byte = 0

            override fun authOptions(
                codegenContext: ClientCodegenContext,
                operationShape: OperationShape,
                baseAuthSchemeOptions: List<AuthSchemeOption>,
            ): List<AuthSchemeOption> =
                baseAuthSchemeOptions +
                    AuthSchemeOption.EndpointBasedAuthSchemeOption
        },
)
