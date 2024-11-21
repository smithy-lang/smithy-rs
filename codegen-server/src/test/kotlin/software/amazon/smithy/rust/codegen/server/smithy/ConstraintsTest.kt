/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.inspectors.forAll
import io.kotest.matchers.ints.shouldBeGreaterThan
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BooleanShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.AbstractTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import java.io.File

enum class ModelProtocol(val trait: AbstractTrait) {
    AwsJson10(AwsJson1_0Trait.builder().build()),
    AwsJson11(AwsJson1_1Trait.builder().build()),
    RestJson(RestJson1Trait.builder().build()),
    RestXml(RestXmlTrait.builder().build()),
    Rpcv2Cbor(Rpcv2CborTrait.builder().build()),
}

/**
 * Returns the Smithy constraints model from the common repository, with the specified protocol
 * applied to the service.
 */
fun loadSmithyConstraintsModelForProtocol(modelProtocol: ModelProtocol): Pair<Model, ShapeId> {
    val (model, serviceShapeId) = loadSmithyConstraintsModel()
    return Pair(model.replaceProtocolTrait(serviceShapeId, modelProtocol), serviceShapeId)
}

/**
 * Loads the Smithy constraints model defined in the common repository and returns the model along with
 * the service shape defined in it.
 */
fun loadSmithyConstraintsModel(): Pair<Model, ShapeId> {
    val filePath = "../codegen-core/common-test-models/constraints.smithy"
    val model =
        File(filePath).readText().asSmithyModel()
    val serviceShapeId = model.shapes().filter { it.isServiceShape }.findFirst().orElseThrow().id
    return Pair(model, serviceShapeId)
}

/**
 * Removes all existing protocol traits annotated on the given service,
 * then sets the provided `protocol` as the sole protocol trait for the service.
 */
fun Model.replaceProtocolTrait(
    serviceShapeId: ShapeId,
    modelProtocol: ModelProtocol,
): Model {
    val serviceBuilder =
        this.expectShape(serviceShapeId, ServiceShape::class.java).toBuilder()
    for (p in ModelProtocol.values()) {
        serviceBuilder.removeTrait(p.trait.toShapeId())
    }
    val service = serviceBuilder.addTrait(modelProtocol.trait).build()
    return ModelTransformer.create().replaceShapes(this, listOf(service))
}

fun List<ShapeId>.containsAnyShapeId(ids: Collection<ShapeId>): Boolean {
    return ids.any { id -> this.any { shape -> shape == id } }
}

/**
 * Removes the given operations from the model.
 */
fun Model.removeOperations(
    serviceShapeId: ShapeId,
    operationsToRemove: List<ShapeId>,
): Model {
    val service = this.expectShape(serviceShapeId, ServiceShape::class.java)
    val serviceBuilder = service.toBuilder()
    // The operation must exist in the service.
    service.operations.map { it.toShapeId() }.containsAll(operationsToRemove) shouldBe true
    // Remove all operations.
    for (opToRemove in operationsToRemove) {
        serviceBuilder.removeOperation(opToRemove)
    }
    val changedModel = ModelTransformer.create().replaceShapes(this, listOf(serviceBuilder.build()))
    // The operation must not exist in the updated service.
    val changedService = changedModel.expectShape(serviceShapeId, ServiceShape::class.java)
    changedService.operations.size shouldBeGreaterThan 0
    changedService.operations.map { it.toShapeId() }.containsAnyShapeId(operationsToRemove) shouldBe false

    return changedModel
}

class ConstraintsTest {
    private val model =
        """
        namespace test

        service TestService {
            version: "123",
            operations: [TestOperation]
        }

        operation TestOperation {
            input: TestInputOutput,
            output: TestInputOutput,
        }

        structure TestInputOutput {
            map: MapA,
            recursive: RecursiveShape
        }

        structure RecursiveShape {
            shape: RecursiveShape,
            mapB: MapB
        }

        @length(min: 1, max: 69)
        map MapA {
            key: String,
            value: MapB
        }

        map MapB {
            key: String,
            value: StructureA
        }

        @uniqueItems
        list ListA {
            member: MyString
        }

        @pattern("\\w+")
        string MyString

        @length(min: 1, max: 69)
        string LengthString

        structure StructureA {
            @range(min: 1, max: 69)
            int: Integer,
            @required
            string: String
        }

        // This shape is not in the service closure.
        structure StructureB {
            @pattern("\\w+")
            patternString: String,
            @required
            requiredString: String,
            mapA: MapA,
            @length(min: 1, max: 5)
            mapAPrecedence: MapA
        }

        structure StructWithInnerDefault {
            @default(false)
            inner: PrimitiveBoolean
        }
        """.asSmithyModel(smithyVersion = "2")
    private val symbolProvider = serverTestSymbolProvider(model)

    private val testInputOutput = model.lookup<StructureShape>("test#TestInputOutput")
    private val recursiveShape = model.lookup<StructureShape>("test#RecursiveShape")
    private val mapA = model.lookup<MapShape>("test#MapA")
    private val mapB = model.lookup<MapShape>("test#MapB")
    private val listA = model.lookup<ListShape>("test#ListA")
    private val lengthString = model.lookup<StringShape>("test#LengthString")
    private val structA = model.lookup<StructureShape>("test#StructureA")
    private val structAInt = model.lookup<MemberShape>("test#StructureA\$int")
    private val structAString = model.lookup<MemberShape>("test#StructureA\$string")
    private val structWithInnerDefault = model.lookup<StructureShape>("test#StructWithInnerDefault")
    private val primitiveBoolean = model.lookup<BooleanShape>("smithy.api#PrimitiveBoolean")

    @Test
    fun `it should detect supported constrained traits as constrained`() {
        listOf(listA, mapA, structA, lengthString).forAll {
            it.isDirectlyConstrained(symbolProvider) shouldBe true
        }
    }

    @Test
    fun `it should not detect unsupported constrained traits as constrained`() {
        listOf(structAInt, structAString).forAll {
            it.isDirectlyConstrained(symbolProvider) shouldBe false
        }
    }

    @Test
    fun `it should evaluate reachability of constrained shapes`() {
        mapA.canReachConstrainedShape(model, symbolProvider) shouldBe true
        structAInt.canReachConstrainedShape(model, symbolProvider) shouldBe false
        listA.canReachConstrainedShape(model, symbolProvider) shouldBe true

        // All of these eventually reach `StructureA`, which is constrained because one of its members is `required`.
        testInputOutput.canReachConstrainedShape(model, symbolProvider) shouldBe true
        mapB.canReachConstrainedShape(model, symbolProvider) shouldBe true
        recursiveShape.canReachConstrainedShape(model, symbolProvider) shouldBe true
    }

    @Test
    fun `it should not consider shapes with the default trait as constrained`() {
        structWithInnerDefault.canReachConstrainedShape(model, symbolProvider) shouldBe false
        primitiveBoolean.isDirectlyConstrained(symbolProvider) shouldBe false
    }

    private fun generateAndCompileServer(
        model: Model,
        pubConstraints: Boolean = true,
        dir: File? = null,
        test: (ServerCodegenContext, RustCrate) -> Unit = { _, _ -> },
    ) {
        if (dir?.exists() == true) {
            dir.deleteRecursively()
        }

        // Simply compiling the crate is sufficient as a test.
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = "test#SampleService",
                additionalSettings =
                    ServerAdditionalSettings()
                        .publicConstrainedTypes(pubConstraints)
                        .build(),
                overrideTestDir = dir,
            ),
            test = test,
        )
    }

    @Test
    fun `unnamed and named enums should validate and have an associated ConstraintViolation error type`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service SampleService {
                operations: [SampleOp]
            }

            @http(uri: "/dailySummary", method: "POST")
            operation SampleOp {
                input := {
                    unnamedDay: UnnamedDayOfWeek
                    namedDay: DayOfWeek
                }
                errors: [ValidationException]
            }
            @enum([
                { value: "MONDAY" },
                { value: "TUESDAY" }
            ])
            string UnnamedDayOfWeek
            @enum([
                { value: "MONDAY", name: "MONDAY" },
                { value: "TUESDAY", name: "TUESDAY" }
            ])
            string DayOfWeek
            """.asSmithyModel(smithyVersion = "2")

        generateAndCompileServer(model) { _, crate ->
            crate.unitTest("value_should_be_validated") {
                rustTemplate(
                    """
                    let x: Result<crate::model::DayOfWeek, crate::model::day_of_week::ConstraintViolation> =
                        "Friday".try_into();
                    assert!(x.is_err());
                    
                    let x: Result<crate::model::UnnamedDayOfWeek, crate::model::unnamed_day_of_week::ConstraintViolation> =
                        "Friday".try_into();
                    assert!(x.is_err());
                """,
                )
            }
        }
    }

    private fun createModel(
        inputMemberShape: String,
        additionalShapes: () -> String,
    ) = """
        namespace test
        use aws.protocols#restJson1
        use smithy.framework#ValidationException

        @restJson1
        service SampleService {
            operations: [SampleOp]
        }

        @http(uri: "/sample", method: "POST")
        operation SampleOp {
            input := {
                items : $inputMemberShape
            }
            errors: [ValidationException]
        }
        @length(min: 0 max: 65535)
        string ItemName
        string ItemDescription
        ${additionalShapes()}
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `constrained map with an indirectly constrained nested list should compile`() {
        val model =
            createModel("ItemMap") {
                """
                @length(min: 1 max: 100)
                map ItemMap {
                    key: ItemName,
                    value: ItemListA
                }
                list ItemListA { 
                    member: ItemListB
                }
                list ItemListB {
                    member: ItemDescription
                }
                """
            }
        generateAndCompileServer(model)
    }

    // TODO(#3895): Move tests that use `generateAndCompileServer` into `constraints.smithy` once issue is resolved.
    @Test
    fun `constrained list with an indirectly constrained map should compile`() {
        val model =
            createModel("ItemList") {
                """
                @length(min: 1 max: 100)
                list ItemList {
                    member: Item
                }
                map Item {
                    key: ItemName
                    value: ItemDescription
                }
                """
            }
        generateAndCompileServer(model)
    }

    @Test
    fun `constrained list with an indirectly constrained nested list should compile`() {
        val model =
            createModel("ItemList") {
                """
                @length(min: 1 max: 100)
                list ItemList {
                    member: ItemA
                }
                list ItemA {
                    member: ItemB
                }
                list ItemB {
                    member: ItemName
                }
                """
            }
        generateAndCompileServer(model)
    }

    @Test
    fun `constrained list with an indirectly constrained list that has an indirectly constrained map should compile`() {
        val model =
            createModel("ItemList") {
                """
                @length(min: 1 max: 100)
                list ItemList {
                    member: NestedItemList
                }
                list NestedItemList {
                    member: Item
                }
                map Item {
                    key: ItemName
                    value: ItemDescription
                }
                """
            }
        generateAndCompileServer(model)
    }
}
