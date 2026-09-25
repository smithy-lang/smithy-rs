/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class RestXmlParserGeneratorTest {
    @Test
    fun `constrained and unconstrained members compile`() {
        val model =
            """
            namespace test

            use aws.protocols#restXml
            use smithy.framework#ValidationException

            @restXml
            service RestXmlConstrained {
                version: "2024-01-01"
                operations: [ConstrainedInput, PayloadOp]
            }

            @http(uri: "/items", method: "POST")
            operation ConstrainedInput {
                input: ConstrainedInputInput
                output: ConstrainedInputOutput
                errors: [ValidationException]
            }

            structure ConstrainedInputInput {
                count: ConstrainedCount
                tag: ConstrainedTag
                labels: ConstrainedLabels
                attributes: ConstrainedAttributes
                nested: NestedHolder
                kind: ConstrainedKind
                choice: AnalyticsFilter

                @xmlFlattened
                rules: LifecycleRules

                @required
                enabled: Boolean

                @required
                threshold: Integer

                @httpHeader("X-Count")
                headerCount: ConstrainedCount

                @httpHeader("X-Tag")
                headerTag: ConstrainedTag

                @httpQuery("count")
                queryCount: ConstrainedCount

                @httpQuery("tag")
                queryTag: ConstrainedTag
            }

            structure ConstrainedInputOutput {}

            @http(uri: "/payload", method: "POST")
            operation PayloadOp {
                input: PayloadOpInput
                output: PayloadOpOutput
                errors: [ValidationException]
            }

            structure PayloadOpInput {
                @httpPayload
                body: NestedHolder
            }

            structure PayloadOpOutput {}

            structure NestedHolder {
                tag: ConstrainedTag

                // Mirrors `s3#Grantee${'$'}Type`: a required XML attribute member targeting a
                // Smithy enum. The attribute path must use the builder setter so the parsed
                // string is lifted into the constrained value.
                @required
                @xmlAttribute
                @xmlName("xsi:type")
                kind: GranteeKind
            }

            enum GranteeKind {
                CANONICAL_USER = "CanonicalUser"
                CUSTOMER_BY_EMAIL = "AmazonCustomerByEmail"
                GROUP = "Group"
            }

            enum ConstrainedKind {
                A
                B
                C
            }

            @range(min: 1, max: 100)
            integer ConstrainedCount

            @pattern("^[A-Za-z0-9]+${'$'}")
            string ConstrainedTag

            @length(min: 1, max: 4)
            list ConstrainedLabels {
                member: ConstrainedTag
            }

            @length(min: 1, max: 4)
            map ConstrainedAttributes {
                key: ConstrainedTag
                value: ConstrainedTag
            }

            union AnalyticsFilter {
                prefix: ConstrainedTag
                tag: NestedHolder
            }

            list LifecycleRules {
                member: LifecycleRule
            }

            structure LifecycleRule {
                id: ConstrainedTag
            }
            """.asSmithyModel(smithyVersion = "2")

        serverIntegrationTest(model) { _, _ -> }
    }
}
