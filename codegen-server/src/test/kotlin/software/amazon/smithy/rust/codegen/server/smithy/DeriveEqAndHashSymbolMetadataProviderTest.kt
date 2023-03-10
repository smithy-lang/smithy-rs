/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.collections.shouldContainAll
import io.kotest.matchers.collections.shouldNotContain
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.MethodSource
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.smithy.BaseSymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import java.util.stream.Stream

internal class DeriveEqAndHashSymbolMetadataProviderTest {
    private val model =
        """
        namespace test

        service TestService {
            version: "123"
            operations: [TestOperation, StreamingOperation, EventStreamOperation]
        }

        operation TestOperation {
            input: TestInputOutput
            output: TestInputOutput
        }

        operation StreamingOperation {
            input: StreamingOperationInputOutput
            output: StreamingOperationInputOutput
        }

        operation EventStreamOperation {
            input: EventStreamOperationInputOutput
            output: EventStreamOperationInputOutput
        }

        structure EventStreamOperationInputOutput {
            @httpPayload
            @required
            union: StreamingUnion
        }

        structure StreamingOperationInputOutput {
            @httpPayload
            @required
            blobStream: BlobStream
        }

        @streaming
        blob BlobStream

        structure TestInputOutput {
            hasFloat: HasFloat
            hasDouble: HasDouble
            hasDocument: HasDocument
            containsFloat: ContainsFloat
            containsDouble: ContainsDouble
            containsDocument: ContainsDocument

            hasList: HasList
            hasListWithMap: HasListWithMap
            hasMap: HasMap

            eqAndHashStruct: EqAndHashStruct
        }

        structure EqAndHashStruct {
            blob: Blob
            boolean: Boolean
            string: String
            byte: Byte
            short: Short
            integer: Integer
            long: Long
            enum: Enum
            timestamp: Timestamp

            list: List
            union: EqAndHashUnion

            // bigInteger: BigInteger
            // bigDecimal: BigDecimal
        }

        list List {
            member: String
        }

        list ListWithMap {
            member: Map
        }

        map Map {
            key: String
            value: String
        }

        union EqAndHashUnion {
            blob: Blob
            boolean: Boolean
            string: String
            byte: Byte
            short: Short
            integer: Integer
            long: Long
            enum: Enum
            timestamp: Timestamp

            list: List
        }

        @streaming
        union StreamingUnion {
            eqAndHashStruct: EqAndHashStruct
        }

        structure HasFloat {
            float: Float
        }

        structure HasDouble {
            double: Double
        }

        structure HasDocument {
            document: Document
        }

        structure HasList {
            list: List
        }

        structure HasListWithMap {
            list: ListWithMap
        }

        structure HasMap {
            map: Map
        }

        structure ContainsFloat {
            hasFloat: HasFloat
        }

        structure ContainsDouble {
            hasDouble: HasDouble
        }

        structure ContainsDocument {
            containsDocument: HasDocument
        }

        enum Enum {
            DIAMOND
            CLUB
            HEART
            SPADE
        }
        """.asSmithyModel(smithyVersion = "2.0")
    private val serviceShape = model.lookup<ServiceShape>("test#TestService")
    private val deriveEqAndHashSymbolMetadataProvider = serverTestSymbolProvider(model, serviceShape)
        .let { BaseSymbolMetadataProvider(it, additionalAttributes = listOf()) }
        .let { DeriveEqAndHashSymbolMetadataProvider(it) }

    companion object {
        @JvmStatic
        fun getShapes(): Stream<Arguments> {
            val shapesWithNeitherEqNorHash = listOf(
                "test#StreamingOperationInputOutput",
                "test#EventStreamOperationInputOutput",
                "test#StreamingUnion",
                "test#BlobStream",
                "test#TestInputOutput",
                "test#HasFloat",
                "test#HasDouble",
                "test#HasDocument",
                "test#ContainsFloat",
                "test#ContainsDouble",
                "test#ContainsDocument",
            )

            val shapesWithEqAndHash = listOf(
                "test#EqAndHashStruct",
                "test#EqAndHashUnion",
                "test#Enum",
                "test#HasList",
            )

            val shapesWithOnlyEq = listOf(
                "test#HasListWithMap",
                "test#HasMap",
            )

            return (
                shapesWithNeitherEqNorHash.map { Arguments.of(it, emptyList<RuntimeType>()) } +
                    shapesWithEqAndHash.map { Arguments.of(it, listOf(RuntimeType.Eq, RuntimeType.Hash)) } +
                    shapesWithOnlyEq.map { Arguments.of(it, listOf(RuntimeType.Eq)) }
                ).stream()
        }
    }

    @ParameterizedTest(name = "(#{index}) Derive `Eq` and `Hash` when possible. Params = shape: {0}, expectedTraits: {1}")
    @MethodSource("getShapes")
    fun `it should derive Eq and Hash when possible`(
        shapeId: String,
        expectedTraits: Collection<RuntimeType>,
    ) {
        val shape = model.lookup<Shape>(shapeId)
        val derives = deriveEqAndHashSymbolMetadataProvider.toSymbol(shape).expectRustMetadata().derives
        derives shouldContainAll expectedTraits
    }

    @ParameterizedTest
    // These don't implement `PartialEq` because they are not constrained, so they don't generate newtypes. If the
    // symbol provider wrapped `ConstrainedShapeSymbolProvider` and they were constrained, they would generate
    // newtypes, and they would hence implement `PartialEq`.
    @ValueSource(strings = ["test#List", "test#Map", "test#ListWithMap", "smithy.api#Blob"])
    fun `it should not derive Eq if shape does not implement PartialEq`(shapeId: String) {
        val shape = model.lookup<Shape>(shapeId)
        val derives = deriveEqAndHashSymbolMetadataProvider.toSymbol(shape).expectRustMetadata().derives
        derives shouldNotContain RuntimeType.PartialEq
        derives shouldNotContain RuntimeType.Eq
    }
}
