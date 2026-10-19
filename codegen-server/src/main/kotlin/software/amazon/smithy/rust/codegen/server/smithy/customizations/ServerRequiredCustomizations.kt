/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.AllowLintsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.CrateVersionCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.pubUseSmithyPrimitives
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerSchemaConstantGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerSchemaGenerator

/**
 * A set of customizations that are included in all protocols.
 *
 * This exists as a convenient place to gather these modifications, these are not true customizations.
 *
 * See [RequiredCustomizations] from the `codegen-client` subproject for the client version of this decorator.
 */
class ServerRequiredCustomizations : ServerCodegenDecorator {
    override val name: String = "ServerRequired"
    override val order: Byte = -1

    override fun libRsCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> =
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/4366) Remove additionalClippyLints once the issue is resolved
        baseCustomizations + AllowLintsCustomization(additionalClippyLints = listOf("uninlined_format_args", "clone_on_copy"))

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig

        // Add rt-tokio feature for `ByteStream::from_path`
        rustCrate.mergeFeature(
            Feature(
                "rt-tokio",
                true,
                listOf("aws-smithy-types/rt-tokio"),
            ),
        )

        // Use version-aware smithy-http-server dependency name for features
        val smithyHttpServerDep = ServerCargoDependency.smithyHttpServer(rc)
        val smithyHttpServerName = smithyHttpServerDep.name

        rustCrate.mergeFeature(
            Feature(
                "aws-lambda",
                false,
                listOf("$smithyHttpServerName/aws-lambda"),
            ),
        )

        rustCrate.mergeFeature(
            Feature(
                "request-id",
                true,
                listOf("$smithyHttpServerName/request-id"),
            ),
        )

        rustCrate.withModule(ServerRustModule.Types) {
            pubUseSmithyPrimitives(codegenContext, codegenContext.model, rustCrate)(this)
            if (codegenContext.serviceShape.hasEventStreamOperations(codegenContext.model)) {
                rustTemplate(
                    "pub use #{EventStreamSender};",
                    "EventStreamSender" to RuntimeType.eventStreamSender(rc),
                )
            }
            rustTemplate(
                """
                pub use #{DisplayErrorContext};
                """,
                "Response" to RuntimeType.smithyHttp(rc).resolve("operation::Response"),
                "DisplayErrorContext" to RuntimeType.smithyTypes(rc).resolve("error::display::DisplayErrorContext"),
            )
        }

        rustCrate.withModule(ServerRustModule.root) {
            CrateVersionCustomization.extras(rustCrate, ServerRustModule.root)
        }
    }
}

/**
 * Generates the `crate::schema` descriptor modules:
 *
 * - `schema::operations` holds one `OperationSchema` static per operation bound to the service, carrying the
 *   operation shape ID, its input and output schemas, and its modeled errors. The operation's `@http` binding is
 *   read from its input schema, where [ServerSchemaGenerator] records it.
 * - `schema::service` holds the `ServiceSchema` static tying the service shape ID, version, protocols and
 *   operations together, with the service's protocol traits (and their members) recorded as typed traits.
 *
 * Shape schemas are reached through the `SCHEMA` constants that [ServerSchemaConstantGenerator] renders next
 * to each generated type.
 */
class ServerServiceSchemaGenerator(
    private val codegenContext: ServerCodegenContext,
    /** Decorator-supplied `traits.insert(...)` statements for the service's trait map. */
    private val serviceTraitInsertions: List<Writable> = emptyList(),
) {
    private val model = codegenContext.model
    private val service = codegenContext.serviceShape
    private val symbolProvider = codegenContext.symbolProvider
    private val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
    private val serverSchema =
        ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType().resolve("schema")
    private val codegenScope =
        arrayOf(
            "OperationSchema" to serverSchema.resolve("OperationSchema"),
            "Schema" to smithySchema.resolve("Schema"),
            "ServiceSchema" to serverSchema.resolve("ServiceSchema"),
            "ShapeId" to smithySchema.resolve("ShapeId"),
            "TraitMap" to smithySchema.resolve("TraitMap"),
            *RuntimeType.preludeScope,
        )
    private val operations = TopDownIndex.of(model).getContainedOperations(service).sortedBy { it.id }
    private val serviceConstName = serviceSchemaConstName(service)

    fun render(rustCrate: RustCrate) {
        rustCrate.withModule(OperationsModule) {
            operations.forEach { renderOperation(this, it) }
        }
        rustCrate.withModule(ServiceModule) {
            renderService(this)
        }
    }

    private fun renderOperation(
        writer: RustWriter,
        operation: OperationShape,
    ) {
        val name = operationConstName(operation)
        val errorRefs = operation.errorsSet.sorted().joinToString(", ") { schemaRef(model.expectShape(it)) }
        writer.rustTemplate(
            """
            static ${name}_ERRORS: &[&#{Schema}<'static>] = &[$errorRefs];

            /// Descriptor for the `${escape(operation.id)}` operation.
            pub(crate) static $name: #{OperationSchema}<'static> = #{OperationSchema}::new(
                ${shapeIdExpr(operation.id)},
                ${schemaRef(operation.inputShape(model))},
                ${schemaRef(operation.outputShape(model))},
                ${name}_ERRORS,
            );
            """,
            *codegenScope,
        )
    }

    private fun renderService(writer: RustWriter) {
        val protocols = codegenContext.servedProtocols.joinToString(", ") { shapeIdExpr(it) }
        val operationRefs = operations.joinToString(", ") { "&super::operations::${operationConstName(it)}" }
        val version = service.version.takeIf { it.isNotEmpty() }?.let { "Some(${it.dq()})" } ?: "None"
        writer.rustTemplate(
            """
            static ${serviceConstName}_PROTOCOLS: &[#{ShapeId}<'static>] = &[$protocols];

            static ${serviceConstName}_OPERATIONS: &[&#{OperationSchema}<'static>] = &[$operationRefs];

            /// The protocol traits applied to the service, with their members as modeled.
            static ${serviceConstName}_TRAITS: ::std::sync::LazyLock<#{TraitMap}> = ::std::sync::LazyLock::new(|| {
                ##[allow(unused_mut)]
                let mut traits = #{TraitMap}::new();
                #{ProtocolTraits}
                #{AdditionalTraits}
                traits
            });

            /// Descriptor for the `${escape(service.id)}` service and the operations bound to it.
            pub static $serviceConstName: #{ServiceSchema}<'static> = #{ServiceSchema}::new(
                ${shapeIdExpr(service.id)},
                $version,
                ${serviceConstName}_PROTOCOLS,
                ${serviceConstName}_OPERATIONS,
            )
            .with_traits(&${serviceConstName}_TRAITS);
            """,
            *codegenScope,
            "ProtocolTraits" to protocolTraits(),
            "AdditionalTraits" to serviceTraitInsertions.join("\n"),
        )
    }

    /**
     * Inserts one typed trait per served built-in protocol, built from the trait's model node so that
     * members left unset stay unset. Decorators record their own protocols' traits through
     * [software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator.schemaTraitInsertions]
     * on the service shape.
     */
    private fun protocolTraits(): Writable =
        writable {
            for (protocol in codegenContext.servedProtocols) {
                val rustType = protocolTraitTypes[protocol] ?: continue
                val node = service.findTrait(protocol).orElse(null)?.toNode()?.expectObjectNode() ?: ObjectNode.objectNode()
                val chain = StringBuilder("#{Trait}::new()")

                fun versions(member: String) =
                    node.getArrayMember(member).orElse(null)?.let { list ->
                        list.elements.joinToString(", ", prefix = "#{Vec}::from([", postfix = "])") {
                            "#{String}::from(${it.expectStringNode().value.dq()})"
                        }
                    }
                versions("http")?.let { chain.append(".with_http($it)") }
                versions("eventStreamHttp")?.let { chain.append(".with_event_stream_http($it)") }
                if (protocol == RestXmlTrait.ID && node.getBooleanMemberOrDefault("noErrorWrapping", false)) {
                    chain.append(".with_no_error_wrapping(true)")
                }
                rustTemplate(
                    "traits.insert(#{Box}::new($chain));",
                    "Trait" to serverSchema.resolve("traits::$rustType"),
                    *RuntimeType.preludeScope,
                )
            }
        }

    private val protocolTraitTypes =
        mapOf(
            AwsJson1_0Trait.ID to "AwsJson1_0Trait",
            AwsJson1_1Trait.ID to "AwsJson1_1Trait",
            RestJson1Trait.ID to "RestJson1Trait",
            RestXmlTrait.ID to "RestXmlTrait",
            Rpcv2CborTrait.ID to "Rpcv2CborTrait",
        )

    private fun operationConstName(operation: OperationShape): String =
        symbolProvider.toSymbol(operation).name.toSnakeCase().uppercase()

    private fun schemaRef(shape: Shape): String = "${symbolProvider.toSymbol(shape).fullName}::SCHEMA"

    private fun shapeIdExpr(id: ShapeId): String =
        """#{ShapeId}::from_parts(${escape(id).dq()}, ${id.namespace.dq()}, ${id.name.dq()})"""

    /** Shape IDs contain `#`, which is the template escape character. */
    private fun escape(id: ShapeId): String = id.toString().replace("#", "##")

    companion object {
        private val SchemaModule =
            RustModule.public("schema", documentationOverride = "Schema descriptors for this service and its operations.")

        /** `crate::schema::operations`: one `OperationSchema` per operation bound to the service. */
        val OperationsModule = RustModule.pubCrate("operations", parent = SchemaModule)

        /** `crate::schema::service`: the `ServiceSchema` for the generated service. */
        val ServiceModule =
            RustModule.public("service", parent = SchemaModule, documentationOverride = "The schema descriptor for this service.")

        /** The name of the `ServiceSchema` static in `crate::schema::service`. */
        fun serviceSchemaConstName(service: ServiceShape): String = service.id.name.toSnakeCase().uppercase()
    }
}
