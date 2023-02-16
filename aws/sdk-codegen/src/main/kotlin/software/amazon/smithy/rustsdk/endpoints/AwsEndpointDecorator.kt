/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.endpoints

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.letIf

class AwsEndpointDecorator : ClientCodegenDecorator {
    override val name: String = "AwsEndpoint"
    override val order: Byte = 100

    override fun transformModel(service: ServiceShape, model: Model): Model {
        val customServices = setOf(
            ShapeId.from("com.amazonaws.s3#AmazonS3"),
            ShapeId.from("com.amazonaws.s3control#AWSS3ControlServiceV20180820"),
            ShapeId.from("com.amazonaws.codecatalyst#CodeCatalyst"),
        )
        if (customServices.contains(service.id)) {
            return model
        }
        // currently, most models incorrectly model region is optional when it is actually required—fix these models:
        return ModelTransformer.create().mapTraits(model) { _, trait ->
            when (trait) {
                is EndpointRuleSetTrait -> {
                    val epRules = EndpointRuleSet.fromNode(trait.ruleSet)
                    val newParameters = Parameters.builder()
                    epRules.parameters.toList()
                        .map { param ->
                            param.letIf(param.builtIn == Builtins.REGION.builtIn) { parameter ->
                                val builder = parameter.toBuilder().required(true)
                                // TODO(https://github.com/awslabs/smithy-rs/issues/2187): undo this workaround
                                parameter.defaultValue.ifPresent { default -> builder.defaultValue(default) }

                                builder.build()
                            }
                        }
                        .forEach(newParameters::addParameter)

                    val newTrait = epRules.toBuilder().parameters(
                        newParameters.build(),
                    ).build()
                    EndpointRuleSetTrait.builder().ruleSet(newTrait.toNode()).build()
                }

                else -> trait
            }
        }
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val epTypes = EndpointTypesGenerator.fromContext(codegenContext)
        if (epTypes.defaultResolver() == null) {
            throw CodegenException(
                "${codegenContext.serviceShape} did not provide endpoint rules. " +
                    "This is a bug and the generated client will not work. All AWS services MUST define endpoint rules.",
            )
        }
    }
}
