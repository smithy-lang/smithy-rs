/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ConditionalDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.AuthSchemeLister
import software.amazon.smithy.rust.codegen.client.smithy.generators.AuthOptionsPluginGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.getTrait

class EndpointBasedAuthSchemeResolverDecorator : ConditionalDecorator(
    predicate = { codegenContext, _ ->
        codegenContext?.let {
            if (codegenContext.serviceShape.id == ShapeId.from("com.amazonaws.s3#AmazonS3")) {
                // S3 refers to s3express in endpoint rule set
                true
            } else {
                // sigv4a may only be listed in endpoint rule set, not in the auth trait
                val endpointAuthSchemes =
                    codegenContext.serviceShape.getTrait<EndpointRuleSetTrait>()?.ruleSet?.let {
                        EndpointRuleSet.fromNode(
                            it,
                        )
                    }
                        ?.also { it.typeCheck() }?.let { AuthSchemeLister.authSchemesForRuleset(it) } ?: setOf()
                endpointAuthSchemes.contains("sigv4a")
            }
        } ?: false
    },
    delegateTo =
        object : ClientCodegenDecorator {
            override val name: String get() = "EndpointBasedAuthSchemeResolverDecorator"
            override val order: Byte = 0

            override fun operationCustomizations(
                codegenContext: ClientCodegenContext,
                operation: OperationShape,
                baseCustomizations: List<OperationCustomization>,
            ): List<OperationCustomization> =
                baseCustomizations +
                    object : OperationCustomization() {
                        override fun section(section: OperationSection) =
                            writable {
                                when (section) {
                                    is OperationSection.AdditionalRuntimePlugins -> {
                                        section.addClientPlugin(
                                            this,
                                            AuthOptionsPluginGenerator(codegenContext).endpointBasedAuthPlugin(
                                                section.operationShape,
                                                section.authSchemeOptions,
                                            ),
                                        )
                                    }

                                    else -> {}
                                }
                            }
                    }
        },
)
