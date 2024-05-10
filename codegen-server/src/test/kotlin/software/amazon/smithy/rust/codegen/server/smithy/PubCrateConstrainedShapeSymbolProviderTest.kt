/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProviders

class PubCrateConstrainedShapeSymbolProviderTest {
    private val model =
        """
        $BASE_MODEL_STRING

        structure NonTransitivelyConstrainedStructureShape {
            constrainedString: ConstrainedString,
            constrainedMap: ConstrainedMap,
            unconstrainedMap: TransitivelyConstrainedMap
        }

        list TransitivelyConstrainedCollection {
            member: Structure
        }

        structure Structure {
            @required
            requiredMember: String
        }

        structure StructureWithMemberTargetingAggregateShape {
            member: TransitivelyConstrainedCollection
        }

        union Union {
            structure: Structure
        }
        """.asSmithyModel()

    private val serverTestSymbolProviders = serverTestSymbolProviders(model)
    private val symbolProvider = serverTestSymbolProviders.symbolProvider
    private val pubCrateConstrainedShapeSymbolProvider = serverTestSymbolProviders.pubCrateConstrainedShapeSymbolProvider

    @Test
    fun `it should crash when provided with a shape that is directly constrained`() {
        val constrainedStringShape = model.lookup<StringShape>("test#ConstrainedString")
        shouldThrow<IllegalArgumentException> {
            pubCrateConstrainedShapeSymbolProvider.toSymbol(constrainedStringShape)
        }
    }

    @Test
    fun `it should crash when provided with a shape that is unconstrained`() {
        val unconstrainedStringShape = model.lookup<StringShape>("test#UnconstrainedString")
        shouldThrow<IllegalArgumentException> {
            pubCrateConstrainedShapeSymbolProvider.toSymbol(unconstrainedStringShape)
        }
    }

    @Test
    fun `it should return an opaque type for transitively constrained collection shapes`() {
        val transitivelyConstrainedCollectionShape = model.lookup<ListShape>("test#TransitivelyConstrainedCollection")
        val transitivelyConstrainedCollectionType =
            pubCrateConstrainedShapeSymbolProvider.toSymbol(transitivelyConstrainedCollectionShape).rustType()

        transitivelyConstrainedCollectionType shouldBe
            RustType.Opaque(
                "TransitivelyConstrainedCollectionConstrained",
                "crate::constrained::transitively_constrained_collection_constrained",
            )
    }

    @Test
    fun `it should return an opaque type for transitively constrained map shapes`() {
        val transitivelyConstrainedMapShape = model.lookup<MapShape>("test#TransitivelyConstrainedMap")
        val transitivelyConstrainedMapType =
            pubCrateConstrainedShapeSymbolProvider.toSymbol(transitivelyConstrainedMapShape).rustType()

        transitivelyConstrainedMapType shouldBe
            RustType.Opaque(
                "TransitivelyConstrainedMapConstrained",
                "crate::constrained::transitively_constrained_map_constrained",
            )
    }

    @Test
    fun `it should not blindly delegate to the base symbol provider when provided with a transitively constrained structure member shape targeting an aggregate shape`() {
        val memberShape = model.lookup<MemberShape>("test#StructureWithMemberTargetingAggregateShape\$member")
        val memberType = pubCrateConstrainedShapeSymbolProvider.toSymbol(memberShape).rustType()

        memberType shouldBe
            RustType.Option(
                RustType.Opaque(
                    "TransitivelyConstrainedCollectionConstrained",
                    "crate::constrained::transitively_constrained_collection_constrained",
                ),
            )
    }

    @Test
    fun `it should delegate to the base symbol provider when provided with a structure shape`() {
        val structureShape = model.lookup<StructureShape>("test#NonTransitivelyConstrainedStructureShape")
        val structureSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(structureShape)

        structureSymbol shouldBe symbolProvider.toSymbol(structureShape)
    }

    @Test
    fun `it should delegate to the base symbol provider when provided with a union shape`() {
        val unionShape = model.lookup<UnionShape>("test#Union")
        val unionSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(unionShape)

        unionSymbol shouldBe symbolProvider.toSymbol(unionShape)
    }
}
