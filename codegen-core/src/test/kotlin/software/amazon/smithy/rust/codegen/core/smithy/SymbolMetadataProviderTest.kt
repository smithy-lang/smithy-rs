/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider

class SymbolMetadataProviderTest {
    private val model =
        """
        namespace test

        @sensitive
        string Secret

        list SecretList {
            member: Secret
        }

        @uniqueItems
        list SecretSet {
            member: Secret
        }

        map SecretKeyMap {
            key: Secret
            value: String
        }

        map SecretValueMap {
            key: String
            value: Secret
        }

        list NestedSecretList {
            member: SecretList
        }

        map MapOfSecretLists {
            key: String
            value: SecretList
        }

        list PlainList {
            member: String
        }

        map PlainMap {
            key: String
            value: PlainList
        }

        structure WithSecretList { public: String, secrets: SecretList }
        structure WithSecretSet { public: String, secrets: SecretSet }
        structure WithSecretKeyMap { public: String, secrets: SecretKeyMap }
        structure WithSecretValueMap { public: String, secrets: SecretValueMap }
        structure WithNestedSecretList { public: String, secrets: NestedSecretList }
        structure WithMapOfSecretLists { public: String, secrets: MapOfSecretLists }
        structure WithDirectSecret { public: String, secret: Secret }

        @sensitive
        structure SensitiveStruct { public: String }

        structure WithPlainCollections { list: PlainList, map: PlainMap }

        structure WithHolder { label: String, holder: WithSecretList }

        union UnionWithSecretList { plain: String, secrets: SecretList }
        union UnionWithSecretValueMap { plain: String, secrets: SecretValueMap }
        union PlainUnion { plain: String, list: PlainList }
        """.asSmithyModel(smithyVersion = "2")

    private val provider = testSymbolProvider(model)

    private fun hasDebugDerive(shapeName: String): Boolean {
        val shape: Shape = model.expectShape(ShapeId.from("test#$shapeName"))
        return provider.toSymbol(shape).expectRustMetadata().hasDebugDerive()
    }

    @ParameterizedTest
    @ValueSource(
        strings = [
            "WithSecretList",
            "WithSecretSet",
            "WithSecretKeyMap",
            "WithSecretValueMap",
            "WithNestedSecretList",
            "WithMapOfSecretLists",
            "WithDirectSecret",
            "SensitiveStruct",
            "UnionWithSecretList",
            "UnionWithSecretValueMap",
        ],
    )
    fun `containers with sensitive descendants in collections do not derive Debug`(shapeName: String) {
        hasDebugDerive(shapeName) shouldBe false
    }

    @ParameterizedTest
    @ValueSource(strings = ["WithPlainCollections", "PlainUnion"])
    fun `containers without sensitive descendants derive Debug`(shapeName: String) {
        hasDebugDerive(shapeName) shouldBe true
    }

    @ParameterizedTest
    @ValueSource(strings = ["WithHolder"])
    fun `containers holding a structure with sensitive descendants still derive Debug`(shapeName: String) {
        // `shouldRedact` does not descend into structures. The derived `Debug` calls the inner
        // structure's own redacting `Debug` impl, so the output is still redacted.
        hasDebugDerive(shapeName) shouldBe true
    }
}
