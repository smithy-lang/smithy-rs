/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.protocols.shapeModuleName
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerSchemaGenerator
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.traits.ShapeReachableFromOperationInputTagTrait

/**
 * Server mirror of the client `SchemaDecorator`: registers [ServerSchemaGenerator]
 * for server shapes in the *error closure* — every operation error shape
 * (including event-stream errors, which the normalizer hoists into operation
 * error enums) plus every structure/union transitively reachable from one.
 *
 * All schema code renders into a dedicated `schema_serde` module, one file per
 * shape (`schema_serde/shape_<name>.rs`, mirroring the `protocol_serde`
 * layout), keeping the shape modules readable. For each shape this emits
 * (serialize-only — the server's Phase 1 schema-serde consumer is error
 * serialization):
 * - the `Schema<'static>` statics and the `SCHEMA` const,
 * - the `SerializableStruct` impl, with member writes in the order the legacy
 *   error serializer used — member-name-sorted for REST protocols, model member
 *   order for RPC protocols — which the byte-identical error-body requirement
 *   hinges on (assumptions register F2 follow-up),
 * - for `@error` shapes, `ModeledError` and `HttpModeledError` impls with the
 *   HTTP status resolved at codegen time (`@httpError` code, else the
 *   `@error` fault default: client = 400, server = 500).
 */
class ServerSchemaDecorator : ServerCodegenDecorator {
    override val name: String = "ServerSchemaDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        // The legacy `aws-smithy-legacy-http-server` fork (http 0.x) does not
        // carry the modeled-error modules; schema generation targets http 1.x.
        if (codegenContext.runtimeConfig.httpVersion != HttpVersion.Http1x) {
            return
        }
        val closure =
            errorClosure(
                codegenContext.model,
                codegenContext.serviceShape,
                codegenContext.symbolProvider,
                codegenContext.settings.codegenConfig.publicConstrainedTypes,
            )
        val schemaSerdeModule = RustModule.pubCrate("schema_serde")
        for (shapeId in closure.sorted()) {
            val shape = codegenContext.model.expectShape(shapeId)
            val shapeModule =
                RustModule.pubCrate(
                    codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, shape),
                    parent = schemaSerdeModule,
                )
            rustCrate.withModule(shapeModule) {
                ServerSchemaGenerator(
                    codegenContext,
                    this,
                    shape,
                    serializeMemberOrder = errorSerializeOrder(codegenContext, shape),
                ).renderSerializeOnly()
                shape.getTrait<ErrorTrait>()?.also { errorTrait ->
                    renderModeledErrorImpls(codegenContext, this, shape, errorTrait)
                }
            }
        }
    }

    /**
     * The member write order the legacy error serializer used, so that
     * schema-driven error bodies stay byte-identical (assumptions register F2
     * follow-up):
     *
     * - REST protocols (restJson1, restXml): `HttpTraitHttpBindingResolver.mappedBindings`
     *   sorts error-response bindings by member name, so the legacy body wrote
     *   document members in member-name order (e.g. `fieldList` before `message`
     *   on `ValidationException`).
     * - RPC protocols (awsJson 1.0/1.1, rpcv2Cbor): `StaticHttpBindingResolver`
     *   binds `shape.members()` verbatim — model member order, the
     *   [ServerSchemaGenerator] default.
     *
     * Only `@error` shapes get the override: legacy *nested* structure
     * serializers always use model member order.
     *
     * Known limitation (multi-protocol services): the legacy per-protocol
     * serializers could order the same shape differently per protocol; a
     * single `serialize_members` impl follows the service's primary protocol.
     */
    private fun errorSerializeOrder(
        codegenContext: ServerCodegenContext,
        shape: Shape,
    ): List<MemberShape>? {
        if (shape !is StructureShape || !shape.hasTrait(ErrorTrait::class.java)) {
            return null
        }
        val restProtocols =
            setOf(
                ShapeId.from("aws.protocols#restJson1"),
                ShapeId.from("aws.protocols#restXml"),
            )
        return if (codegenContext.protocol in restProtocols) {
            shape.members().sortedBy { it.memberName }
        } else {
            null
        }
    }

    private fun renderModeledErrorImpls(
        codegenContext: ServerCodegenContext,
        writer: RustWriter,
        shape: Shape,
        errorTrait: ErrorTrait,
    ) {
        val smithyHttpServer = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
        val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
        val fullName = codegenContext.symbolProvider.toSymbol(shape).fullName
        // `@httpError` code if present, else the `@error` fault default
        // (client = 400, server = 500) — smithy-java's
        // `ModeledException.getHttpStatusCode` semantics, applied once at
        // build time.
        val status =
            shape.getTrait<HttpErrorTrait>()?.code
                ?: if (errorTrait.isClientError) 400 else 500
        writer.rustTemplate(
            """
            impl #{ModeledError} for $fullName {
                fn schema(&self) -> &#{Schema}<'_> {
                    Self::SCHEMA
                }
            }
            impl #{HttpModeledError} for $fullName {
                fn status_code(&self) -> u16 {
                    $status
                }
            }
            """,
            "ModeledError" to smithyHttpServer.resolve("modeled_error::ModeledError"),
            "HttpModeledError" to smithyHttpServer.resolve("modeled_error::HttpModeledError"),
            "Schema" to smithySchema.resolve("Schema"),
        )
    }

    companion object {
        /**
         * The set of structure/union shapes reachable from any *schema-safe*
         * operation error of [service] (the error shapes themselves included).
         *
         * Under `publicConstrainedTypes=true`, constrained shapes reachable
         * from operation input generate as newtype wrappers, which the
         * serialize-only schema pass does not handle beyond strings (whose
         * newtypes expose `as_str()`). An error whose closure reaches any
         * other constrained newtype — a `@range` number, `@length` blob or
         * collection, or a constrained string inside a list/map — is excluded
         * wholesale, so the generated crate always compiles. The constrained
         * story is deferred to the RFC's `publicConstrainedTypes` migration
         * (section 6).
         */
        fun errorClosure(
            model: Model,
            service: ServiceShape,
            symbolProvider: SymbolProvider,
            publicConstrainedTypes: Boolean,
        ): Set<ShapeId> {
            val walker = Walker(model)
            val errorShapes: Set<ShapeId> =
                walker.walkShapes(service)
                    .filterIsInstance<OperationShape>()
                    .flatMap { it.errors }
                    .toSet()
            val closure = mutableSetOf<ShapeId>()
            for (errorId in errorShapes) {
                val errorClosure = walker.walkShapes(model.expectShape(errorId))
                val safe =
                    !publicConstrainedTypes ||
                        errorClosure.none { unsafeForSchemaSerialization(model, it, symbolProvider) }
                if (safe) {
                    errorClosure.forEach { shape: Shape ->
                        if (shape is StructureShape || shape is UnionShape) {
                            closure.add(shape.id)
                        }
                    }
                }
            }
            return closure
        }

        /**
         * True when [shape] generates as a constrained newtype (or an aggregate
         * of constrained newtypes) that the serialize-only schema pass cannot
         * drive. Strings are exempt when targeted directly by structure/union
         * members (`as_str()` handles their newtypes) but not as list/map
         * elements, where the specialized slice/map write helpers expect the
         * plain types.
         */
        private fun unsafeForSchemaSerialization(
            model: Model,
            shape: Shape,
            symbolProvider: SymbolProvider,
        ): Boolean {
            fun constrainedNonEnum(target: Shape): Boolean =
                target.hasTrait(ShapeReachableFromOperationInputTagTrait::class.java) &&
                    target.isDirectlyConstrained(symbolProvider) &&
                    target !is EnumShape &&
                    !target.hasTrait(EnumTrait::class.java)
            return when (shape) {
                is StructureShape, is UnionShape -> false
                is ListShape ->
                    constrainedNonEnum(shape) || constrainedNonEnum(model.expectShape(shape.member.target))
                is MapShape ->
                    constrainedNonEnum(shape) ||
                        constrainedNonEnum(model.expectShape(shape.key.target)) ||
                        constrainedNonEnum(model.expectShape(shape.value.target))
                is StringShape -> false // newtypes expose as_str()
                is MemberShape, is OperationShape, is ServiceShape -> false
                else -> constrainedNonEnum(shape)
            }
        }
    }
}
