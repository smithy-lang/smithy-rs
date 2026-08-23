/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.HttpErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import java.util.Collections
import java.util.IdentityHashMap

/**
 * Server mirror of the client `SchemaDecorator`: registers [SchemaGenerator]
 * for server shapes in the *error closure* — every operation error shape
 * (including event-stream errors, which the normalizer hoists into operation
 * error enums) plus every structure/union transitively reachable from one.
 *
 * For each shape in the closure this emits (serialize-only — the server's
 * Phase 1 schema-serde consumer is error serialization):
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

    override fun structureCustomizations(
        codegenContext: software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> =
        // The legacy `aws-smithy-legacy-http-server` fork (http 0.x) does not
        // carry the modeled-error modules; schema generation targets http 1.x.
        if (codegenContext.runtimeConfig.httpVersion == HttpVersion.Http1x) {
            baseCustomizations + ServerSchemaStructureCustomization(codegenContext)
        } else {
            baseCustomizations
        }

    companion object {
        // Keyed by model instance identity: one model per codegen run, and the
        // customization is re-created for every structure shape.
        private val errorClosureCache: MutableMap<Model, Set<ShapeId>> =
            Collections.synchronizedMap(IdentityHashMap())

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
            symbolProvider: software.amazon.smithy.codegen.core.SymbolProvider,
            publicConstrainedTypes: Boolean,
        ): Set<ShapeId> =
            errorClosureCache.getOrPut(model) {
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
                closure
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
            symbolProvider: software.amazon.smithy.codegen.core.SymbolProvider,
        ): Boolean {
            fun constrainedNonEnum(target: Shape): Boolean =
                target.hasTrait(
                    software.amazon.smithy.rust.codegen.server.smithy.traits
                        .ShapeReachableFromOperationInputTagTrait::class.java,
                ) &&
                    target.isDirectlyConstrained(symbolProvider) &&
                    !(target is software.amazon.smithy.model.shapes.EnumShape) &&
                    !target.hasTrait(software.amazon.smithy.model.traits.EnumTrait::class.java)
            return when (shape) {
                is StructureShape, is UnionShape -> false
                is software.amazon.smithy.model.shapes.ListShape ->
                    constrainedNonEnum(shape) || constrainedNonEnum(model.expectShape(shape.member.target))
                is software.amazon.smithy.model.shapes.MapShape ->
                    constrainedNonEnum(shape) ||
                        constrainedNonEnum(model.expectShape(shape.key.target)) ||
                        constrainedNonEnum(model.expectShape(shape.value.target))
                is software.amazon.smithy.model.shapes.StringShape -> false // newtypes expose as_str()
                is software.amazon.smithy.model.shapes.MemberShape, is OperationShape, is ServiceShape -> false
                else -> constrainedNonEnum(shape)
            }
        }
    }
}

private class ServerSchemaStructureCustomization(
    private val codegenContext: CodegenContext,
) : StructureCustomization() {
    private val errorClosure: Set<ShapeId> =
        ServerSchemaDecorator.errorClosure(
            codegenContext.model,
            codegenContext.serviceShape,
            codegenContext.symbolProvider,
            (codegenContext as software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext)
                .settings.codegenConfig.publicConstrainedTypes,
        )

    override fun section(section: StructureSection): Writable =
        when (section) {
            is StructureSection.AdditionalTraitImpls ->
                writable {
                    if (section.shape.id in errorClosure) {
                        SchemaGenerator(
                            codegenContext,
                            this,
                            section.shape,
                            serializeMemberOrder = errorSerializeOrder(section.shape),
                        ).renderSerializeOnly()
                        section.shape.getTrait<ErrorTrait>()?.also { errorTrait ->
                            renderModeledErrorImpls(this, section, errorTrait)
                        }
                    }
                }
            else -> emptySection
        }

    /**
     * The member write order the legacy error serializer used, so that
     * schema-driven error bodies stay byte-identical (assumptions register F2):
     *
     * - REST protocols (restJson1, restXml): `HttpTraitHttpBindingResolver.mappedBindings`
     *   sorts error-response bindings by member name, so the legacy body wrote
     *   document members in member-name order (e.g. `fieldList` before `message`
     *   on `ValidationException`).
     * - RPC protocols (awsJson 1.0/1.1, rpcv2Cbor): `StaticHttpBindingResolver`
     *   binds `shape.members()` verbatim — model member order, the
     *   `SchemaGenerator` default.
     *
     * Only `@error` shapes get the override: legacy *nested* structure
     * serializers always use model member order.
     *
     * Known limitation (multi-protocol services): the legacy per-protocol
     * serializers could order the same shape differently per protocol; a
     * single `serialize_members` impl follows the service's primary protocol.
     */
    private fun errorSerializeOrder(shape: StructureShape): List<software.amazon.smithy.model.shapes.MemberShape>? {
        if (!shape.hasTrait(ErrorTrait::class.java)) {
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
        writer: software.amazon.smithy.rust.codegen.core.rustlang.RustWriter,
        section: StructureSection.AdditionalTraitImpls,
        errorTrait: ErrorTrait,
    ) {
        val smithyHttpServer = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
        val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
        // `@httpError` code if present, else the `@error` fault default
        // (client = 400, server = 500) — smithy-java's
        // `ModeledException.getHttpStatusCode` semantics, applied once at
        // build time.
        val status =
            section.shape.getTrait<HttpErrorTrait>()?.code
                ?: if (errorTrait.isClientError) 400 else 500
        writer.rustTemplate(
            """
            impl #{ModeledError} for ${section.structName} {
                fn schema(&self) -> &#{Schema}<'_> {
                    Self::SCHEMA
                }
            }
            impl #{HttpModeledError} for ${section.structName} {
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
}
