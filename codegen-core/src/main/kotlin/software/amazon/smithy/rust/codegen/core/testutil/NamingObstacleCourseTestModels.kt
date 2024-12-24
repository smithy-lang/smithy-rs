/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

object NamingObstacleCourseTestModels {
    private val rustPrelude = preludeScope.map { pair -> pair.first }

    /**
     * Test model that confounds the generation machinery by using operations named after every item
     * in the Rust prelude.
     */
    fun rustPreludeOperationsModel(): Model =
        StringBuilder().apply {
            append(
                """
                ${"$"}version: "2.0"
                namespace crate

                use smithy.test#httpRequestTests
                use smithy.test#httpResponseTests
                use aws.protocols#awsJson1_1
                use aws.api#service
                use smithy.framework#ValidationException

                structure InputAndOutput {}

                @awsJson1_1
                @service(sdkId: "Config")
                service Config {
                    version: "2006-03-01",
                    rename: { "smithy.api#String": "PreludeString" },
                    operations: [
                """,
            )
            for (item in rustPrelude) {
                append("$item,\n")
            }
            append(
                """
                ]
                }
                """,
            )
            for (item in rustPrelude) {
                append("operation $item { input: InputAndOutput, output: InputAndOutput, errors: [ValidationException] }\n")
            }
        }.toString().asSmithyModel()

    fun rustPreludeStructsModel(): Model =
        StringBuilder().apply {
            append(
                """
                ${"$"}version: "2.0"
                namespace crate

                use smithy.test#httpRequestTests
                use smithy.test#httpResponseTests
                use aws.protocols#awsJson1_1
                use aws.api#service
                use smithy.framework#ValidationException

                structure InputAndOutput {}

                @awsJson1_1
                @service(sdkId: "Config")
                service Config {
                    version: "2006-03-01",
                    rename: { "smithy.api#String": "PreludeString" },
                    operations: [
                """,
            )
            for (item in rustPrelude) {
                append("Use$item,\n")
            }
            append(
                """
                ]
                }
                """,
            )
            for (item in rustPrelude) {
                append("structure $item { $item: smithy.api#String }\n")
                append("operation Use$item { input: $item, output: $item, errors: [ValidationException] }\n")
            }
            println(toString())
        }.toString().asSmithyModel()

    fun rustPreludeEnumsModel(): Model =
        StringBuilder().apply {
            append(
                """
                ${"$"}version: "2.0"
                namespace crate

                use smithy.test#httpRequestTests
                use smithy.test#httpResponseTests
                use aws.protocols#awsJson1_1
                use aws.api#service
                use smithy.framework#ValidationException

                structure InputAndOutput {}

                @awsJson1_1
                @service(sdkId: "Config")
                service Config {
                    version: "2006-03-01",
                    rename: { "smithy.api#String": "PreludeString" },
                    operations: [
                """,
            )
            for (item in rustPrelude) {
                append("Use$item,\n")
            }
            append(
                """
                ]
                }
                """,
            )
            for (item in rustPrelude) {
                append("enum $item { $item }\n")
                append("structure Struct$item { $item: $item }\n")
                append("operation Use$item { input: Struct$item, output: Struct$item, errors: [ValidationException] }\n")
            }
        }.toString().asSmithyModel()

    fun rustPreludeEnumVariantsModel(): Model =
        StringBuilder().apply {
            append(
                """
                ${"$"}version: "2.0"
                namespace crate

                use smithy.test#httpRequestTests
                use smithy.test#httpResponseTests
                use aws.protocols#awsJson1_1
                use aws.api#service
                use smithy.framework#ValidationException

                @awsJson1_1
                @service(sdkId: "Config")
                service Config {
                    version: "2006-03-01",
                    rename: { "smithy.api#String": "PreludeString" },
                    operations: [EnumOp]
                }

                operation EnumOp {
                    input: InputAndOutput,
                    output: InputAndOutput,
                    errors: [ValidationException],
                }

                structure InputAndOutput {
                    the_enum: TheEnum,
                }

                enum TheEnum {
                """,
            )
            for (item in rustPrelude) {
                append("$item,\n")
            }
            append(
                """
                }
                """,
            )
        }.toString().asSmithyModel()

    /**
     * This targets two bug classes:
     * - operation inputs used as nested outputs
     * - operation outputs used as nested inputs
     */
    fun reusedInputOutputShapesModel(protocol: Trait) =
        """
        namespace test
        use ${protocol.toShapeId()}
        use aws.api#service
        @${protocol.toShapeId().name}
        @service(sdkId: "test")
        service Service {
            version: "2006-03-01",
            operations: [GetThing, ReuseGetThingIO]
        }

        // re-use get thing output in a list & in an operation
        @http(uri: "/SomeOperation2", method: "POST")
        operation GetThing {
            output: GetThingOutput
            input: GetThingInput
        }

        // an operation that re-uses the input and output shapes from `GetThing` above. this has caused issues in the
        // past with operation/input shape confusion during function signature generation
        @http(uri: "/SomeOperation3", method: "POST")
        operation ReuseGetThingIO {
            input: GetThingNested
            output: GetThingNested
        }

        structure GetThingOutput {
            @required
            meta: String
        }

        structure GetThingInput {
            @required
            meta: String
        }

        // nested structure which reuses input and output shapes internally
        structure GetThingNested {
            thingsOut: GetThingOutputList,
            thingsIn: GetThingInputList,
            thingOut: GetThingOutput,
            thingIn: GetThingInput
        }

        list GetThingOutputList {
            member: GetThingOutput
        }
        list GetThingInputList {
            member: GetThingInput
        }
        """.asSmithyModel()
}
