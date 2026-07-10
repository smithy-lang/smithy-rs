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
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

/**
 * [RecursiveShapeClassifier] answers, for a nested list/map element, whether
 * emitting that element's resolved schema constant would close a cycle in the
 * inline `static` schema data. Reachability is computed over aggregate edges
 * only (list element, map key/value); structure and union targets terminate the
 * walk because they carry their own `::SCHEMA` constant.
 */
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

        // Inner doesn't reach back to Outer.
        classifier.isRecursive(outer, inner) shouldBe false
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

        classifier.isRecursive(a, b) shouldBe false
    }

    @Test
    fun `nested aggregate reaching its container only through a struct is not an aggregate cycle`() {
        // OuterList -> (element) InnerMap -> (value) Wrapper -> OuterList. The only
        // path from InnerMap back to OuterList passes through the struct Wrapper,
        // which carries its own `::SCHEMA` constant, so emitting OuterList's
        // resolved `_MEMBER` schema for the InnerMap element does NOT close a
        // static-data cycle. The element keeps its resolved schema (and member
        // traits such as `@xmlName`).
        val model =
            """
            namespace com.example
            structure Wrapper { values: OuterList }
            list OuterList { member: InnerMap }
            map InnerMap { key: String, value: Wrapper }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val outerList = model.lookup<ListShape>("com.example#OuterList")
        val innerMap = model.lookup<MapShape>("com.example#InnerMap")

        classifier.isRecursive(outerList, innerMap) shouldBe false
    }

    @Test
    fun `nested aggregate reaching its container only through a union is not an aggregate cycle`() {
        // Mirrors DynamoDB's AttributeValue. A list of AttributeMap: the map's
        // value is the union AttributeValue, which loops back to AttributeMap.
        // That loop passes through the union (its own `::SCHEMA` constant and
        // write-site handling), so writing the AttributeMap element of
        // AttributeMapList is not a cycle back to the list.
        val model =
            """
            namespace com.example
            list AttributeMapList { member: AttributeMap }
            map AttributeMap { key: String, value: AttributeValue }
            union AttributeValue { S: String, M: AttributeMap }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val attributeMapList = model.lookup<ListShape>("com.example#AttributeMapList")
        val attributeMap = model.lookup<MapShape>("com.example#AttributeMap")

        classifier.isRecursive(attributeMapList, attributeMap) shouldBe false
    }

    @Test
    fun `struct and union element targets terminate aggregate reachability`() {
        // Mutually recursive structs and a recursive list-of-struct: none of these
        // are aggregate cycles, because the recursion is carried by the
        // structs/unions (handled by RecursiveShapeBoxer and the named `::SCHEMA`
        // constants), not by aggregate edges.
        val model =
            """
            namespace com.example
            structure A { b: B }
            structure B { a: A }
            structure Tree { children: TreeList }
            list TreeList { member: Tree }
            """.asSmithyModel()
        val classifier = RecursiveShapeClassifier(model)
        val a = model.lookup<StructureShape>("com.example#A")
        val b = model.lookup<StructureShape>("com.example#B")
        val tree = model.lookup<StructureShape>("com.example#Tree")
        val treeList = model.lookup<ListShape>("com.example#TreeList")

        classifier.isRecursive(a, b) shouldBe false
        classifier.isRecursive(b, a) shouldBe false
        // The list element is the struct Tree; a struct target terminates the walk.
        classifier.isRecursive(treeList, tree) shouldBe false
    }

    // -- Aggregate-only cycles (the `true` / defensive-guard path) -------------
    //
    // Smithy's model validation rejects aggregate-only cycles (recursion must
    // pass through a structure or union), so a *valid* model can never drive
    // `isRecursive` to `true`. These tests build such invalid models with
    // `asSmithyModel(disableValidation = true)` to exercise the guard branch and
    // the `seen`-set termination directly — proving the classifier is total and
    // that the `SchemaGenerator` `prelude::DOCUMENT` fallback it gates is
    // reachable exactly (and only) for these otherwise-illegal shapes.

    @Test
    fun `self-referential list is an aggregate cycle`() {
        // `list SelfList { member: SelfList }` is an aggregate-only cycle Smithy
        // would reject; validation is disabled so we can classify it. The
        // aggregate closure includes the start shape, so a self-referential
        // aggregate is recursive.
        val model =
            """
            namespace com.example
            list SelfList { member: SelfList }
            """.asSmithyModel(disableValidation = true)
        val classifier = RecursiveShapeClassifier(model)
        val selfList = model.lookup<ListShape>("com.example#SelfList")

        classifier.isRecursive(selfList, selfList) shouldBe true
    }

    @Test
    fun `self-referential map value is an aggregate cycle`() {
        // `map SelfMap { key: String, value: SelfMap }` — same idea via a map
        // value edge.
        val model =
            """
            namespace com.example
            map SelfMap { key: String, value: SelfMap }
            """.asSmithyModel(disableValidation = true)
        val classifier = RecursiveShapeClassifier(model)
        val selfMap = model.lookup<MapShape>("com.example#SelfMap")

        classifier.isRecursive(selfMap, selfMap) shouldBe true
    }

    @Test
    fun `two-node list-map aggregate cycle is recursive in both directions`() {
        // A -> (element) B -> (value) A, with no intervening struct/union. This
        // is the multi-hop aggregate-only cycle; without the `seen` set the walk
        // would not terminate. Both member-target edges close the cycle.
        val model =
            """
            namespace com.example
            list A { member: B }
            map B { key: String, value: A }
            """.asSmithyModel(disableValidation = true)
        val classifier = RecursiveShapeClassifier(model)
        val a = model.lookup<ListShape>("com.example#A")
        val b = model.lookup<MapShape>("com.example#B")

        // A's member edge targets B, and B reaches back to A (B -> value A).
        classifier.isRecursive(a, b) shouldBe true
        // B's value edge targets A, and A reaches back to B (A -> member B).
        classifier.isRecursive(b, a) shouldBe true
    }
}
