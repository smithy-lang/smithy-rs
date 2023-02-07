/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientRestXmlFactory
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.traits.AllowInvalidXmlRoot
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.endpoints.stripEndpointTrait
import software.amazon.smithy.rustsdk.getBuiltIn
import software.amazon.smithy.rustsdk.toWritable
import java.util.logging.Logger

/**
 * Top level decorator for S3
 */
class S3Decorator : ClientCodegenDecorator {
    override val name: String = "S3"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)
    private val invalidXmlRootAllowList = setOf(
        // API returns GetObjectAttributes_Response_ instead of Output
        ShapeId.from("com.amazonaws.s3#GetObjectAttributesOutput"),
    )

    private fun applies(serviceId: ShapeId) =
        serviceId == ShapeId.from("com.amazonaws.s3#AmazonS3")

    override fun protocols(
        serviceId: ShapeId,
        currentProtocols: ProtocolMap<ClientProtocolGenerator, ClientCodegenContext>,
    ): ProtocolMap<ClientProtocolGenerator, ClientCodegenContext> =
        currentProtocols.letIf(applies(serviceId)) {
            it + mapOf(
                RestXmlTrait.ID to ClientRestXmlFactory { protocolConfig ->
                    S3(protocolConfig)
                },
            )
        }

    override fun transformModel(service: ServiceShape, model: Model): Model {
        return model.letIf(applies(service.id)) {
            ModelTransformer.create().mapShapes(model) { shape ->
                shape.letIf(isInInvalidXmlRootAllowList(shape)) {
                    logger.info("Adding AllowInvalidXmlRoot trait to $it")
                    (it as StructureShape).toBuilder().addTrait(AllowInvalidXmlRoot()).build()
                }
            }.let(StripBucketFromHttpPath()::transform).let(stripEndpointTrait("RequestRoute"))
        }
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations.letIf(applies(codegenContext.serviceShape.id)) {
        it + S3PubUse()
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf(object : EndpointCustomization {
            override fun setBuiltInOnServiceConfig(name: String, value: Node, configBuilderRef: String): Writable? {
                if (!name.startsWith("AWS::S3")) {
                    return null
                }
                val builtIn = codegenContext.getBuiltIn(name) ?: return null
                return writable {
                    rustTemplate(
                        "let $configBuilderRef = $configBuilderRef.${builtIn.name.rustName()}(#{value});",
                        "value" to value.toWritable(),
                    )
                }
            }
        },
        )
    }

    private fun isInInvalidXmlRootAllowList(shape: Shape): Boolean {
        return shape.isStructureShape && invalidXmlRootAllowList.contains(shape.id)
    }
}

class S3(codegenContext: CodegenContext) : RestXml(codegenContext) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val errorScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "Error" to RuntimeType.genericError(runtimeConfig),
        "HeaderMap" to RuntimeType.HttpHeaderMap,
        "Response" to RuntimeType.HttpResponse,
        "XmlDecodeError" to RuntimeType.smithyXml(runtimeConfig).resolve("decode::XmlDecodeError"),
        "base_errors" to restXmlErrors,
        "s3_errors" to AwsRuntimeType.S3Errors,
    )

    override fun parseHttpGenericError(operationShape: OperationShape): RuntimeType {
        return RuntimeType.forInlineFun("parse_http_generic_error", RustModule.private("xml_deser")) {
            rustBlockTemplate(
                "pub fn parse_http_generic_error(response: &#{Response}<#{Bytes}>) -> Result<#{Error}, #{XmlDecodeError}>",
                *errorScope,
            ) {
                rustTemplate(
                    """
                    if response.body().is_empty() {
                        let mut err = #{Error}::builder();
                        if response.status().as_u16() == 404 {
                            err.code("NotFound");
                        }
                        Ok(err.build())
                    } else {
                        let base_err = #{base_errors}::parse_generic_error(response.body().as_ref())?;
                        Ok(#{s3_errors}::parse_extended_error(base_err, response.headers()))
                    }
                    """,
                    *errorScope,
                )
            }
        }
    }
}

class S3PubUse : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable = when (section) {
        is LibRsSection.Body -> writable {
            rust(
                "pub use #T::ErrorExt;",
                AwsRuntimeType.S3Errors,
            )
        }

        else -> emptySection
    }
}
