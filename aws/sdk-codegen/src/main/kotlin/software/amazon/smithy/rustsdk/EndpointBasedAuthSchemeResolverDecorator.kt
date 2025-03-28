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

/**
 * Contains a list of SDK IDs that are allowed to use EndpointBasedAuthSchemeOptionResolver
 *
 * Going forward, we expect new services to leverage static information in a model as much as possible (e.g., the auth
 * trait). It helps avoid runtime factors, such as endpoints, influencing auth option resolution behavior.
 */
private val EndpointBasedAuthSchemeAllowList =
    setOf(
        "CloudFront KeyValueStore",
        "EventBridge",
        "S3",
        "SESv2",
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
