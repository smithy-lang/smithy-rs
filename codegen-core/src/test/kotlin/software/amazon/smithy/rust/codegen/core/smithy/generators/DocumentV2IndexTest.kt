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
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.DocumentV2OnlyTrait
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.expectMember
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
            docs: DocumentList
            sparseDocs: SparseDocumentList
            documentMap: DocumentMap
            nestedDocuments: DocumentMapList
            data: Data
            nestedChoice: NestedChoice
            text: String
        }

        union NestedChoice {
            choice: Choice
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
    fun `union members holding documents get additive V2 variants`() {
        val choice = model.lookup<UnionShape>("com.example#Choice")
        val variants = index.unionVariants(choice, symbolProvider)

        variants.map { it.member.memberName } shouldBe
            listOf("doc", "docs", "documentMap", "nestedDocuments", "sparseDocs")
        variants.associate { it.member.memberName to it.variantName } shouldBe
            mapOf(
                "doc" to "DocV2",
                "docs" to "DocsV2",
                "documentMap" to "DocumentMapV2",
                "nestedDocuments" to "NestedDocumentsV2",
                "sparseDocs" to "SparseDocsV2",
            )
        variants.associate { it.member.memberName to it.accessorName } shouldBe
            mapOf(
                "doc" to "doc_v2",
                "docs" to "docs_v2",
                "documentMap" to "document_map_v2",
                "nestedDocuments" to "nested_documents_v2",
                "sparseDocs" to "sparse_docs_v2",
            )
        variants.associate { it.member.memberName to it.type.render() } shouldBe
            mapOf(
                "doc" to "::aws_smithy_types::DocumentV2",
                "docs" to "::std::vec::Vec::<::aws_smithy_types::DocumentV2>",
                "documentMap" to
                    "::std::collections::HashMap::<::std::string::String, ::aws_smithy_types::DocumentV2>",
                "nestedDocuments" to
                    "::std::vec::Vec::<::std::collections::HashMap::<::std::string::String, " +
                    "::aws_smithy_types::DocumentV2>>",
                "sparseDocs" to
                    "::std::vec::Vec::<::std::option::Option<::aws_smithy_types::DocumentV2>>",
            )
    }

    @Test
    fun `union traversal stops at structure and union boundaries`() {
        val choice = model.lookup<UnionShape>("com.example#Choice")
        index.unionVariants(choice, symbolProvider).map { it.member.memberName } shouldBe
            listOf("doc", "docs", "documentMap", "nestedDocuments", "sparseDocs")

        val nestedChoice = model.lookup<UnionShape>("com.example#NestedChoice")
        index.unionVariants(nestedChoice, symbolProvider) shouldBe emptyList()
    }

    @Test
    fun `reachability finds affected unions through structures, lists, and other unions`() {
        val nestedChoice = model.lookup<UnionShape>("com.example#NestedChoice")
        index.hasUnionVariants(model.lookup<UnionShape>("com.example#Choice")) shouldBe true
        index.hasUnionVariants(nestedChoice) shouldBe false

        // `NestedChoice` reaches `Choice` through its own member, and `Holder` through a structure member.
        index.reachesUnionVariants(nestedChoice) shouldBe true
        index.reachesUnionVariants(model.lookup<StructureShape>("com.example#Holder")) shouldBe true
        index.reachesUnionVariants(model.lookup<StructureShape>("com.example#NoDocuments")) shouldBe true
        // A structure with only direct documents has companions but no affected union.
        index.reachesUnionVariants(model.lookup<StructureShape>("com.example#Data")) shouldBe false
    }

    @Test
    fun `a synthetic union variant collision is an error`() {
        val model =
            """
            namespace com.example
            union Choice {
                doc: Document
                docV2: String
            }
            """.asSmithyModel()
        val choice = model.lookup<UnionShape>("com.example#Choice")

        val error =
            assertThrows<CodegenException> {
                DocumentV2Index.of(model).unionVariants(choice, testSymbolProvider(model))
            }
        error.message!! shouldContain
            "`com.example#Choice` member `doc` needs a DocumentV2 variant named `DocV2`, but member `docV2` " +
            "already uses that name"
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

    private val mixedModel =
        DocumentV2OnlyTrait.markMembers(
            """
            namespace com.example

            structure Holder {
                old: Document
                new: Document
                newList: DocumentList
                data: Data
            }

            structure Data {
                new: Document
            }

            union Choice {
                old: Document
                new: Document
                text: String
            }

            union NewOnlyChoice {
                new: Document
            }

            structure HoldsNewOnlyChoice {
                choice: NewOnlyChoice
            }

            structure NoDocuments {
                name: String
            }

            list DocumentList { member: Document }
            """.asSmithyModel(smithyVersion = "2.0"),
            listOf(
                "com.example#Holder\$new",
                "com.example#Holder\$newList",
                "com.example#Data\$new",
                "com.example#Choice\$new",
                "com.example#NewOnlyChoice\$new",
            ).map(ShapeId::from),
        )
    private val mixedIndex = DocumentV2Index.of(mixedModel)
    private val mixedSymbolProvider = testSymbolProvider(mixedModel)

    @Test
    fun `members that use DocumentV2 only get no companion or additive variant`() {
        val holder = mixedModel.lookup<StructureShape>("com.example#Holder")
        mixedIndex.companions(holder, mixedSymbolProvider).map { it.member.memberName } shouldBe listOf("old")
        mixedIndex.v2OnlyMembers(holder).map { it.memberName } shouldBe listOf("new", "newList")
        mixedIndex.isV2Only(holder.expectMember("old")) shouldBe false
        mixedIndex.isV2Only(holder.expectMember("new")) shouldBe true
        mixedIndex.hasV2OnlyMembers() shouldBe true

        // A structure whose only document member uses `DocumentV2` only has no companions.
        val data = mixedModel.lookup<StructureShape>("com.example#Data")
        mixedIndex.hasCompanions(data) shouldBe false
        mixedIndex.companions(data, mixedSymbolProvider) shouldBe emptyList()

        val choice = mixedModel.lookup<UnionShape>("com.example#Choice")
        mixedIndex.unionVariants(choice, mixedSymbolProvider).map { it.variantName } shouldBe listOf("OldV2")
        mixedIndex.v2OnlyMembers(choice).map { it.memberName } shouldBe listOf("new")

        index.hasV2OnlyMembers() shouldBe false
    }

    @Test
    fun `only additive variants make a response depend on the union decode mode`() {
        val newOnlyChoice = mixedModel.lookup<UnionShape>("com.example#NewOnlyChoice")
        mixedIndex.hasUnionVariants(newOnlyChoice) shouldBe false
        mixedIndex.reachesUnionVariants(newOnlyChoice) shouldBe false
        mixedIndex.reachesUnionVariants(mixedModel.lookup<StructureShape>("com.example#HoldsNewOnlyChoice")) shouldBe
            false
        mixedIndex.reachesUnionVariants(mixedModel.lookup<UnionShape>("com.example#Choice")) shouldBe true
    }

    @Test
    fun `reachability finds members that use DocumentV2 only through aggregates`() {
        mixedIndex.reachesV2OnlyMembers(mixedModel.lookup<StructureShape>("com.example#Data")) shouldBe true
        mixedIndex.reachesV2OnlyMembers(mixedModel.lookup<StructureShape>("com.example#HoldsNewOnlyChoice")) shouldBe
            true
        mixedIndex.reachesV2OnlyMembers(mixedModel.lookup<StructureShape>("com.example#NoDocuments")) shouldBe false
        // The document target itself isn't a member, so only its containing member is.
        mixedIndex.reachesV2OnlyMembers(mixedModel.expectShape(ShapeId.from("smithy.api#Document"))) shouldBe false
    }

    @Test
    fun `the V2-only type replaces each document`() {
        val holder = mixedModel.lookup<StructureShape>("com.example#Holder")
        val member = holder.expectMember("newList")
        DocumentV2Index.documentV2Type(
            mixedSymbolProvider.toSymbol(member).rustType(),
            member,
            mixedSymbolProvider.config.runtimeConfig,
        ).render() shouldBe "::std::option::Option<::std::vec::Vec::<::aws_smithy_types::DocumentV2>>"
    }

    @Test
    fun `only members that directly hold documents can use DocumentV2 only`() {
        val model =
            """
            namespace com.example
            structure Holder {
                name: String
                data: Data
            }
            structure Data {
                body: Document
            }
            list DocumentList { member: Document }
            """.asSmithyModel()

        for (member in listOf("com.example#Holder\$name", "com.example#Holder\$data", "com.example#DocumentList\$member")) {
            val marked = DocumentV2OnlyTrait.markMembers(model, listOf(ShapeId.from(member)))
            val error = assertThrows<CodegenException> { DocumentV2Index.of(marked) }
            error.message!! shouldContain "`$member` is marked as using `DocumentV2` only"
        }
    }
}
