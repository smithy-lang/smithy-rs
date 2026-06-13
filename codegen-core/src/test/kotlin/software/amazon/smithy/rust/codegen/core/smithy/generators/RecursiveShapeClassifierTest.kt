/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

internal class RecursiveShapeClassifierTest {
    @Test
    fun `non-recursive nested struct returns false`() {
        val model =
            """
            namespace com.example
            structure Outer { inner: Inner }
            structure Inner { name: String }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val outer = model.lookup<StructureShape>("com.example#Outer")
        val inner = model.lookup<StructureShape>("com.example#Inner")

        // Codegen renders Outer's `inner` member targeting Inner. No cycle:
        // Inner doesn't reach back to Outer.
        classifier.isRecursive(outer, inner) shouldBe false
    }

    @Test
    fun `self-recursive struct via a list returns true in both directions`() {
        val model =
            """
            namespace com.example
            structure Tree { children: TreeList }
            list TreeList { member: Tree }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val tree = model.lookup<StructureShape>("com.example#Tree")
        val treeList = model.lookup<ListShape>("com.example#TreeList")

        // The list's element schema is recursive w.r.t. the list itself.
        classifier.isRecursive(treeList, tree) shouldBe true
        // The struct contains a list that recurses back to it.
        classifier.isRecursive(tree, treeList) shouldBe true
    }

    @Test
    fun `mutually recursive structs return true in both directions`() {
        val model =
            """
            namespace com.example
            structure A { b: B }
            structure B { a: A }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val a = model.lookup<StructureShape>("com.example#A")
        val b = model.lookup<StructureShape>("com.example#B")

        classifier.isRecursive(a, b) shouldBe true
        classifier.isRecursive(b, a) shouldBe true
    }

    @Test
    fun `sibling references do not count as recursive`() {
        // A has two members both targeting B, but B doesn't reach back to A.
        val model =
            """
            namespace com.example
            structure A { b1: B, b2: B }
            structure B { name: String }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val a = model.lookup<StructureShape>("com.example#A")
        val b = model.lookup<StructureShape>("com.example#B")

        // Codegen renders A's b1 / b2 members targeting B. No cycle.
        classifier.isRecursive(a, b) shouldBe false
    }

    @Test
    fun `attribute-value-style recursive map`() {
        // Mirrors DynamoDB's AttributeValue: a union with a map member
        // whose value type is the union itself. This is the canonical case
        // where Phase 2 keeps emitting `prelude::DOCUMENT` instead of the
        // resolved value schema.
        val model =
            """
            namespace com.example
            union AttributeValue {
                S: String,
                M: AttributeMap
            }
            map AttributeMap {
                key: String,
                value: AttributeValue
            }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val attrValue = model.lookup<UnionShape>("com.example#AttributeValue")
        val attrMap = model.lookup<MapShape>("com.example#AttributeMap")

        // The map's value schema is recursive w.r.t. the map.
        classifier.isRecursive(attrMap, attrValue) shouldBe true
        // The union contains a map that recurses back to it.
        classifier.isRecursive(attrValue, attrMap) shouldBe true
    }
}
