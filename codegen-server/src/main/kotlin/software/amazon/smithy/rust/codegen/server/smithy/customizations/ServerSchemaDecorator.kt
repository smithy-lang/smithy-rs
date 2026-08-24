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
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerSchemaDeserializerGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerSchemaEventStreamGenerator
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
 * - the `SerializableStruct` impl, writing members in canonical model order
 *   (plan 2e — the legacy REST member-name sort is deliberately gone; restJson1
 *   error-body gates are parse-equal at the top level),
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
        // NOTE: the error closure renders on every http-1.x crate, NOT just
        // `schemaSerde: true` opt-ins. The validation-rejection seam (plan 2d)
        // has the runtime's `RequestRejection::ConstraintViolation` carry
        // `Box<dyn HttpModeledError + Send>` on all http-1.x protocols, so the
        // modeled validation error (an operation error, hence in this closure)
        // must be schema-serializable even when the crate otherwise serves the
        // legacy code paths.
        val errorShapes =
            errorClosure(
                codegenContext.model,
                codegenContext.serviceShape,
                codegenContext.symbolProvider,
                codegenContext.settings.codegenConfig.publicConstrainedTypes,
            )

        // Flag-on crates additionally get the FULL closure of every schema-supported
        // operation (inputs, outputs, and everything transitively reachable), the
        // deserialization walkers for the input side, and the `DeserializableShape`
        // seam on operation inputs (plan Step 4.1/4.2).
        val schemaSerde = codegenContext.settings.codegenConfig.schemaSerde
        val walker = Walker(codegenContext.model)
        val deserClosure = mutableSetOf<ShapeId>()
        val supportedInputShapes = mutableSetOf<ShapeId>()
        val serializeClosure = errorShapes.toMutableSet()
        // Event-stream unions needing frame serde: unions reachable from an
        // operation INPUT get an `Unmarshaller<P>`, from an OUTPUT a
        // `Marshaller<P>` (+ error marshaller). Streaming unions get no schema
        // walker — the unmarshaller drives the EVENT structures' walkers.
        val unmarshallerUnions = mutableSetOf<ShapeId>()
        val marshallerUnions = mutableSetOf<ShapeId>()
        if (schemaSerde) {
            val supportedOps =
                schemaSupportedOperations(
                    codegenContext.model,
                    codegenContext.serviceShape,
                    codegenContext.symbolProvider,
                    codegenContext.settings.codegenConfig.publicConstrainedTypes,
                )
            for (op in supportedOps) {
                val inputShape = op.inputShape(codegenContext.model)
                supportedInputShapes.add(inputShape.id)
                walker.walkShapes(inputShape).forEach { reachable: Shape ->
                    if (ServerSchemaEventStreamGenerator.isEventStreamUnion(reachable)) {
                        unmarshallerUnions.add(reachable.id)
                        // Client-sent modeled stream errors unmarshal through the
                        // error structures' walkers.
                        reachable.asUnionShape().get()
                            .expectTrait(
                                software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait::class.java,
                            )
                            .errorMembers
                            .forEach { errorMember ->
                                walker.walkShapes(codegenContext.model.expectShape(errorMember.target))
                                    .forEach { errShape: Shape ->
                                        if ((errShape is StructureShape || errShape is UnionShape) &&
                                            errShape.id != ShapeId.from("smithy.api#Unit") &&
                                            !ServerSchemaEventStreamGenerator.isEventStreamUnion(errShape)
                                        ) {
                                            deserClosure.add(errShape.id)
                                        }
                                    }
                            }
                    }
                    // `smithy.api#Unit` needs no walker: unit union variants read the
                    // empty struct inline. Streaming unions get no walker either —
                    // frames unmarshal through the event structures' walkers.
                    if ((reachable is StructureShape || reachable is UnionShape) &&
                        reachable.id != ShapeId.from("smithy.api#Unit") &&
                        !ServerSchemaEventStreamGenerator.isEventStreamUnion(reachable)
                    ) {
                        deserClosure.add(reachable.id)
                    }
                }
                walker.walkShapes(op.outputShape(codegenContext.model)).forEach { reachable: Shape ->
                    if (ServerSchemaEventStreamGenerator.isEventStreamUnion(reachable)) {
                        marshallerUnions.add(reachable.id)
                        // The hoisted stream-error structures must be schema-
                        // serializable for the error marshaller.
                        reachable.asUnionShape().get()
                            .expectTrait(
                                software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticEventStreamUnionTrait::class.java,
                            )
                            .errorMembers
                            .forEach { errorMember ->
                                walker.walkShapes(codegenContext.model.expectShape(errorMember.target))
                                    .forEach { errShape: Shape ->
                                        if (errShape is StructureShape || errShape is UnionShape) {
                                            serializeClosure.add(errShape.id)
                                        }
                                    }
                            }
                    }
                }
                walker.walkShapes(op).forEach { reachable: Shape ->
                    if (reachable is StructureShape || reachable is UnionShape) {
                        serializeClosure.add(reachable.id)
                    }
                }
            }
        }

        val schemaSerdeModule = RustModule.pubCrate("schema_serde")
        for (shapeId in (serializeClosure + deserClosure).sorted()) {
            val shape = codegenContext.model.expectShape(shapeId)
            val shapeModule =
                RustModule.pubCrate(
                    codegenContext.symbolProvider.shapeModuleName(codegenContext.serviceShape, shape),
                    parent = schemaSerdeModule,
                )
            rustCrate.withModule(shapeModule) {
                // Member write order is canonical MODEL order everywhere (plan 2e):
                // the legacy REST-protocol member-name sort was protocol knowledge
                // baked into codegen and is deliberately gone. restJson1 error-body
                // gates compare parse-equal at the top level; RPC protocols already
                // used model order and stay byte-exact.
                ServerSchemaGenerator(
                    codegenContext,
                    this,
                    shape,
                ).renderSerializeOnly()
                shape.getTrait<ErrorTrait>()?.also { errorTrait ->
                    renderModeledErrorImpls(codegenContext, this, shape, errorTrait)
                }
                if (shapeId in deserClosure) {
                    val deserGenerator = ServerSchemaDeserializerGenerator(codegenContext, this, shape)
                    deserGenerator.render()
                    if (shapeId in supportedInputShapes) {
                        deserGenerator.renderDeserializableShapeImpl()
                    }
                }
                if (shape is UnionShape &&
                    (shapeId in marshallerUnions || shapeId in unmarshallerUnions)
                ) {
                    val eventGenerator = ServerSchemaEventStreamGenerator(codegenContext, this, shape)
                    if (shapeId in marshallerUnions) {
                        eventGenerator.renderMarshallers()
                    }
                    if (shapeId in unmarshallerUnions) {
                        eventGenerator.renderUnmarshaller()
                    }
                }
            }
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
         * The operations of [service] the schema-driven pipeline can fully serve, both
         * directions (plan Step 4.7's "supported closure"):
         *
         * EVERY operation is schema-supported (plan Step 4.7): streaming-blob and
         * event-stream operations are served through specialized generated glue
         * (splice / `Marshaller<P>`+`Unmarshaller<P>`, plan Step 4.8), still generic
         * over the protocol; constrained newtypes (`publicConstrainedTypes=true`)
         * are fully handled by the schema serializer (`.0` / `as_str()` unwrapping)
         * and the walker (unconstrained parse types).
         */
        fun schemaSupportedOperations(
            model: Model,
            service: ServiceShape,
            symbolProvider: SymbolProvider,
            publicConstrainedTypes: Boolean,
        ): List<OperationShape> {
            val walker = Walker(model)
            return walker.walkShapes(service)
                .filterIsInstance<OperationShape>()
                .sortedBy { it.id }
        }

        /**
         * The set of structure/union shapes reachable from any operation error of
         * [service] (the error shapes themselves included). Constrained newtypes in
         * error closures are handled by the schema serializer's `.0` / `as_str()`
         * unwrapping — no exclusions remain.
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
                walker.walkShapes(model.expectShape(errorId)).forEach { shape: Shape ->
                    if (shape is StructureShape || shape is UnionShape) {
                        closure.add(shape.id)
                    }
                }
            }
            return closure
        }

    }
}
