/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Inspects generated output schemas for sensitivity inherited from scalar, list, map, map-key,
 * and map-value target shapes. It verifies that @sensitive metadata remains attached at the
 * appropriate schema level and that ordinary public headers are not marked sensitive. This
 * protects the metadata used to redact header values in runtime diagnostics.
 */
internal class ServerSchemaSensitiveHeadersTest {
    @Test
    fun `header schemas preserve sensitivity inherited from targets`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.sensitiveheaders
            use aws.protocols#restJson1
            @restJson1
            service Secrets { version: "1", operations: [Read, ReadValues, ReadKeys] }
            @http(uri: "/", method: "GET")
            operation Read { output: Headers }
            @http(uri: "/values", method: "GET")
            operation ReadValues { output: ValueHeaders }
            @http(uri: "/keys", method: "GET")
            operation ReadKeys { output: KeyHeaders }
            structure Headers {
                @httpHeader("x-scalar") scalar: Secret
                @httpHeader("x-list") list: SecretList
                @httpHeader("x-elements") elements: SecretsList
                @httpPrefixHeaders("x-map-") map: SecretMap
                @httpHeader("x-public") public: String
            }
            structure ValueHeaders { @httpPrefixHeaders("x-values-") values: SecretValues }
            structure KeyHeaders { @httpPrefixHeaders("x-keys-") keys: SecretKeys }
            @sensitive string Secret
            @sensitive list SecretList { member: String }
            list SecretsList { member: Secret }
            @sensitive map SecretMap { key: String, value: String }
            map SecretValues { key: String, value: Secret }
            map SecretKeys { key: Secret, value: String }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode(),
                cargoCommand = "cargo test --quiet",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { _, crate ->
            crate.testModule {
                unitTest("inherited_sensitive_header_schemas") {
                    rustTemplate(
                        """
                        let schema = crate::output::ReadOutput::SCHEMA;
                        for name in ["scalar", "list", "map"] {
                            assert!(schema.member_schema(name).unwrap().sensitive().is_some(), "{name}");
                        }
                        assert!(schema.member_schema("elements").unwrap().member().unwrap().sensitive().is_some());
                        let values = crate::output::ReadValuesOutput::SCHEMA.member_schema("values").unwrap();
                        assert!(values.member().unwrap().sensitive().is_some());
                        let keys = crate::output::ReadKeysOutput::SCHEMA.member_schema("keys").unwrap();
                        assert!(keys.key().unwrap().sensitive().is_some());
                        assert!(schema.member_schema("public").unwrap().sensitive().is_none());
                        """,
                        *preludeScope,
                    )
                }
            }
        }
    }
}
