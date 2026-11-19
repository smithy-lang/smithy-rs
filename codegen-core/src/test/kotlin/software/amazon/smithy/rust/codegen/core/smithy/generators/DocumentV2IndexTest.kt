/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.lookup

internal class DocumentV2IndexTest {
    private val model =
        """
        namespace com.example

        structure Holder {
            payload: Document
            @required
            requiredPayload: Document
            list: DocumentList
            sparseList: SparseDocumentList
            map: DocumentMap
            nested: DocumentMapList
            build: Document
            type: Document
            data: Data
            dataList: DataList
            choice: Choice
            name: String
        }

        structure Data {
            body: Document
        }

        structure NoDocuments {
            data: Data
            choice: Choice
        }

        union Choice {
            doc: Document
            text: String
        }

        list DocumentList { member: Document }
        @sparse list SparseDocumentList { member: Document }
        map DocumentMap { key: String, value: Document }
        list DocumentMapList { member: DocumentMap }
        list DataList { member: Data }
        """.asSmithyModel(smithyVersion = "2.0")

    private val symbolProvider =
        testSymbolProvider(
            model,
            rustReservedWordConfig = StructureGeneratorTest.rustReservedWordConfig,
            nullabilityCheckMode = NullableIndex.CheckMode.CLIENT_CAREFUL,
        )
    private val index = DocumentV2Index.of(model)

    @Test
    fun `members holding documents through lists and maps get companions`() {
        val holder = model.lookup<StructureShape>("com.example#Holder")
        val companions = index.companions(holder, symbolProvider)

        companions.map { it.member.memberName } shouldBe
            listOf("payload", "requiredPayload", "list", "sparseList", "map", "nested", "build", "type")
        companions.associate { it.member.memberName to it.type.render() } shouldBe
            mapOf(
                "payload" to "::aws_smithy_types::DocumentV2",
                "requiredPayload" to "::aws_smithy_types::DocumentV2",
                "list" to "::std::vec::Vec::<::aws_smithy_types::DocumentV2>",
                "sparseList" to
                    "::std::vec::Vec::<::std::option::Option<::aws_smithy_types::DocumentV2>>",
                "map" to
                    "::std::collections::HashMap::<::std::string::String, ::aws_smithy_types::DocumentV2>",
                "nested" to
                    "::std::vec::Vec::<::std::collections::HashMap::<::std::string::String, " +
                    "::aws_smithy_types::DocumentV2>>",
                "build" to "::aws_smithy_types::DocumentV2",
                "type" to "::aws_smithy_types::DocumentV2",
            )
        index.hasCompanions(holder) shouldBe true
    }

    @Test
    fun `companion names follow the member's accessor names`() {
        val holder = model.lookup<StructureShape>("com.example#Holder")
        val companions = index.companions(holder, symbolProvider).associateBy { it.member.memberName }

        companions.getValue("requiredPayload").let {
            it.memberName shouldBe "required_payload"
            it.name shouldBe "required_payload_v2"
            it.setterName shouldBe "set_required_payload_v2"
            it.getterName shouldBe "get_required_payload_v2"
        }
        // `build` is renamed to `build_value`, but its setter stays `set_build`.
        companions.getValue("build").let {
            it.memberName shouldBe "build_value"
            it.name shouldBe "build_value_v2"
            it.setterName shouldBe "set_build_v2"
            it.getterName shouldBe "get_build_v2"
        }
        // `type` is a raw identifier, which the `_v2` name doesn't need.
        companions.getValue("type").let {
            it.memberName shouldBe "r##type"
            it.unescapedMemberName shouldBe "type"
            it.name shouldBe "type_v2"
            it.setterName shouldBe "set_type_v2"
        }
    }

    @Test
    fun `structure and union members don't get companions`() {
        val noDocuments = model.lookup<StructureShape>("com.example#NoDocuments")
        index.hasCompanions(noDocuments) shouldBe false
        index.companions(noDocuments, symbolProvider) shouldBe emptyList()

        val data = model.lookup<StructureShape>("com.example#Data")
        index.companions(data, symbolProvider).map { it.name } shouldBe listOf("body_v2")
    }

    @Test
    fun `a companion name that a member already uses is an error`() {
        val model =
            """
            namespace com.example
            structure Holder {
                payload: Document
                payloadV2: String
            }
            """.asSmithyModel()
        val holder = model.lookup<StructureShape>("com.example#Holder")

        val error =
            assertThrows<CodegenException> {
                DocumentV2Index.of(model).companions(holder, testSymbolProvider(model))
            }
        error.message!! shouldContain "`com.example#Holder` member `payload` needs a `DocumentV2` accessor named " +
            "`payload_v2`, but member `payloadV2` already uses that name"
    }

    @Test
    fun `a companion setter name that a member already uses is an error`() {
        val model =
            """
            namespace com.example
            structure Holder {
                payload: Document
                @documentation("Its setter is `set_set_payload_v2`, but its value method is `set_payload_v2`.")
                setPayloadV2: String
            }
            """.asSmithyModel()
        val holder = model.lookup<StructureShape>("com.example#Holder")

        val error =
            assertThrows<CodegenException> {
                DocumentV2Index.of(model).companions(holder, testSymbolProvider(model))
            }
        error.message!! shouldContain "accessor named `set_payload_v2`, but member `setPayloadV2`"
    }
}
