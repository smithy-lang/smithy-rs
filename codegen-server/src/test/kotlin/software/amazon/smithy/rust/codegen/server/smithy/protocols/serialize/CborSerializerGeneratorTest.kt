/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolFunctions
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RpcV2Cbor
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolTestGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.util.function.Predicate

internal class CborSerializerGeneratorTest {
    class DeriveSerdeDeserializeSymbolMetadataProvider(
        private val base: RustSymbolProvider,
    ) : SymbolMetadataProvider(base) {
        private val serdeDeserialize =
            CargoDependency.Serde.copy(scope = DependencyScope.Compile).toType().resolve("Deserialize")

        private fun addDeriveSerdeDeserialize(shape: Shape): RustMetadata {
            check(shape !is MemberShape)

            val baseMetadata = base.toSymbol(shape).expectRustMetadata()
            return baseMetadata.withDerives(serdeDeserialize)
        }

        override fun memberMeta(memberShape: MemberShape) = base.toSymbol(memberShape).expectRustMetadata()

        override fun structureMeta(structureShape: StructureShape) = addDeriveSerdeDeserialize(structureShape)
        override fun unionMeta(unionShape: UnionShape) = addDeriveSerdeDeserialize(unionShape)
        override fun enumMeta(stringShape: StringShape) = addDeriveSerdeDeserialize(stringShape)

        override fun listMeta(listShape: ListShape): RustMetadata = addDeriveSerdeDeserialize(listShape)
        override fun mapMeta(mapShape: MapShape): RustMetadata = addDeriveSerdeDeserialize(mapShape)
        override fun stringMeta(stringShape: StringShape): RustMetadata = addDeriveSerdeDeserialize(stringShape)
        override fun numberMeta(numberShape: NumberShape): RustMetadata = addDeriveSerdeDeserialize(numberShape)
        override fun blobMeta(blobShape: BlobShape): RustMetadata = addDeriveSerdeDeserialize(blobShape)
    }

    @Test
    fun `we serialize and serde_cbor deserializes round trip`() {
        val addDeriveSerdeSerializeDecorator = object : ServerCodegenDecorator {
            override val name: String = "Add `#[derive(serde::Deserialize)]`"
            override val order: Byte = 0

            override fun symbolProvider(base: RustSymbolProvider): RustSymbolProvider =
                DeriveSerdeDeserializeSymbolMetadataProvider(base)
        }

        // Filter out `timestamp` and `blob` shapes: those map to runtime types in `aws-smithy-types` on
        // which we can't `#[derive(serde::Deserialize)]`.
        val model = Model.assembler().discoverModels().assemble().result.get()
        val removeTimestampAndBlobShapes: Predicate<Shape> = Predicate { shape ->
            when (shape) {
                is MemberShape -> {
                    val targetShape = model.expectShape(shape.target)
                    targetShape is BlobShape || targetShape is TimestampShape
                }
                is BlobShape, is TimestampShape -> true
                else -> false
            }
        }
        val transformedModel = ModelTransformer.create().removeShapesIf(model, removeTimestampAndBlobShapes)

        serverIntegrationTest(
            transformedModel,
            additionalDecorators = listOf(addDeriveSerdeSerializeDecorator),
            params = IntegrationTestParams(service = "smithy.protocoltests.rpcv2Cbor#RpcV2Protocol")
        ) { codegenContext, rustCrate ->
            val codegenScope = arrayOf(
                "AssertEq" to RuntimeType.PrettyAssertions.resolve("assert_eq!"),
                "SerdeCbor" to CargoDependency.SerdeCbor.toType(),
            )

            val instantiator = ServerInstantiator(codegenContext, ignoreMissingMembers = true)
            val rpcV2 = RpcV2Cbor(codegenContext)

            for (operationShape in codegenContext.model.operationShapes) {
                val outputShape = operationShape.outputShape(codegenContext.model)
                // TODO Use `httpRequestTests` and error tests too.
                val tests = operationShape.getTrait<HttpResponseTestsTrait>()
                    ?.getTestCasesFor(AppliesTo.SERVER).orEmpty().map {
                        ServerProtocolTestGenerator.TestCase.ResponseTest(
                            it,
                            outputShape,
                        )
                    }

                val serializeFn = rpcV2
                    .structuredDataSerializer()
                    .operationOutputSerializer(operationShape)
                    ?: continue // Skip if there's nothing to serialize.

                // TODO Filter out `timestamp` and `blob` shapes: those map to runtime types in `aws-smithy-types` on
                //  which we can't `#[derive(Deserialize)]`.
                rustCrate.withModule(ProtocolFunctions.serDeModule) {
                    for (test in tests) {
                        unitTest("we_serialize_and_serde_cbor_deserializes_${test.id}") {
                            rustTemplate(
                                """
                                let expected = #{InstantiateShape:W};
                                let bytes = #{SerializeFn}(&expected)
                                    .expect("generated CBOR serializer failed");
                                let actual = #{SerdeCbor}::from_slice(&bytes)
                                   .expect("serde_cbor failed deserializing from bytes");
                                #{AssertEq}(expected, actual);
                                """,
                                "InstantiateShape" to instantiator.generate(test.targetShape, test.testCase.params),
                                "SerializeFn" to serializeFn,
                                *codegenScope,
                            )
                        }
                    }
                }
            }
        }
    }
}
