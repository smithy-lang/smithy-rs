/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
package software.amazon.smithy.rustsdk.customize.route53

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rustsdk.InlineAwsDependency
import java.util.logging.Logger

val Route53: ShapeId = ShapeId.from("com.amazonaws.route53#AWSDnsV20130401")

class Route53Decorator : RustCodegenDecorator {
    override val name: String = "Route53"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)

    private fun applies(service: ServiceShape) = service.id == Route53
    override fun transformModel(service: ServiceShape, model: Model): Model {
        return model.letIf(applies(service)) {
            ModelTransformer.create().mapShapes(model) { shape ->
                shape.letIf(isHostId(shape)) {
                    logger.info("Adding TrimHostedZone trait to $shape")
                    (shape as MemberShape).toBuilder().addTrait(TrimHostedZone()).build()
                }
            }
        }
    }

    override fun operationCustomizations(
        codegenContext: CodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        val hostedZoneMember = operation.inputShape(codegenContext.model).members().find { it.hasTrait<TrimHostedZone>() }
        return if (hostedZoneMember != null) {
            baseCustomizations + TrimHostedZoneCustomization(codegenContext.symbolProvider.toMemberName(hostedZoneMember))
        } else baseCustomizations
    }

    private fun isHostId(shape: Shape): Boolean {
        return (shape is MemberShape && shape.target == ShapeId.from("com.amazonaws.route53#ResourceId")) && shape.hasTrait<HttpLabelTrait>()
    }
}

class TrimHostedZoneCustomization(private val fieldName: String) : OperationCustomization() {
    override fun mutSelf(): Boolean = true
    override fun consumesSelf(): Boolean = true

    private val trimZone =
        RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("hosted_zone_preprocessor"))
            .member("trim_hosted_zone")

    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateInput -> writable {
                rustTemplate("#{trim_hosted_zone}(&mut ${section.input}.$fieldName);", "trim_hosted_zone" to trimZone)
            }
            else -> emptySection
        }
    }
}
