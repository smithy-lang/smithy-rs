/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk.customize.route53

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rustsdk.InlineAwsDependency
import java.util.logging.Logger

class Route53Decorator : ClientCodegenDecorator {
    override val name: String = "Route53"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)
    private val resourceShapes =
        setOf(ShapeId.from("com.amazonaws.route53#ResourceId"), ShapeId.from("com.amazonaws.route53#ChangeId"))

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(isResourceId(shape)) {
                logger.info("Adding TrimResourceId trait to $shape")
                (shape as MemberShape).toBuilder().addTrait(TrimResourceId()).build()
            }
        }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        val inputShape = operation.inputShape(codegenContext.model)
        val hostedZoneMember = inputShape.members().find { it.hasTrait<TrimResourceId>() }
        return if (hostedZoneMember != null) {
            baseCustomizations +
                TrimResourceIdCustomization(
                    codegenContext,
                    inputShape,
                    codegenContext.symbolProvider.toMemberName(hostedZoneMember),
                )
        } else {
            baseCustomizations
        }
    }

    private fun isResourceId(shape: Shape): Boolean {
        return (shape is MemberShape && resourceShapes.contains(shape.target)) && shape.hasTrait<HttpLabelTrait>()
    }
}

class TrimResourceIdCustomization(
    private val codegenContext: ClientCodegenContext,
    private val inputShape: StructureShape,
    private val fieldName: String,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            when (section) {
                is OperationSection.AdditionalInterceptors -> {
                    section.registerInterceptor(codegenContext.runtimeConfig, this) {
                        val smithyRuntimeApi = RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                        val interceptor =
                            RuntimeType.forInlineDependency(
                                InlineAwsDependency.forRustFile("route53_resource_id_preprocessor"),
                            ).resolve("Route53ResourceIdInterceptor")
                        rustTemplate(
                            """
                            #{Route53ResourceIdInterceptor}::new(|input: &mut #{Input}| {
                                &mut input.$fieldName
                            })
                            """,
                            "Input" to codegenContext.symbolProvider.toSymbol(inputShape),
                            "Route53ResourceIdInterceptor" to interceptor,
                            "SharedInterceptor" to smithyRuntimeApi.resolve("client::interceptors::SharedInterceptor"),
                        )
                    }
                }
                else -> {}
            }
        }
}
