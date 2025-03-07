/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class FluentClientGeneratorTest {
    val model =
        """
        namespace com.example
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }

        @optionalAuth
        operation SayHello { input: TestInput }
        structure TestInput {
           foo: String,
           byteValue: Byte,
           listValue: StringList,
           mapValue: ListMap,
           doubleListValue: DoubleList
        }

        list StringList {
            member: String
        }

        list DoubleList {
            member: StringList
        }

        map ListMap {
            key: String,
            value: StringList
        }
        """.asSmithyModel()

    @Test
    fun `generate correct input docs`() {
        val expectations =
            mapOf(
                "listValue" to "list_value(impl Into<String>)",
                "doubleListValue" to "double_list_value(Vec::<String>)",
                "mapValue" to "map_value(impl Into<String>, Vec::<String>)",
                "byteValue" to "byte_value(i8)",
            )
        expectations.forEach { (name, expect) ->
            val member = model.lookup<MemberShape>("com.example#TestInput\$$name")
            member.asFluentBuilderInputDoc(testSymbolProvider(model)) shouldBe expect
        }
    }

    @Test
    fun `send() future implements Send`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("send_future_is_send") {
                val moduleName = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    fn check_send<T: Send>(_: T) {}

                    ##[test]
                    fn test() {
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(#{NeverClient}::new())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        check_send(client.say_hello().send());
                    }
                    """,
                    "NeverClient" to
                        CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig).toType()
                            .resolve("test_util::NeverClient"),
                )
            }
        }
    }

    @Test
    fun `generate inner builders`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("inner_builder") {
                val moduleName = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    ##[test]
                    fn test() {
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(#{NeverClient}::new())
                            .build();
                        let client = $moduleName::Client::from_conf(config);

                        let say_hello_fluent_builder = client.say_hello().byte_value(4).foo("hello!");
                        assert_eq!(*say_hello_fluent_builder.get_foo(), Some("hello!".to_string()));
                        let input = say_hello_fluent_builder.as_input();
                        assert_eq!(*input.get_byte_value(), Some(4));
                    }
                    """,
                    "NeverClient" to
                        CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig).toType()
                            .resolve("test_util::NeverClient"),
                )
            }
        }
    }

    @Test
    fun `dead-code warning should not be issued when a service has no operations`() {
        val model =
            """
            namespace com.example
            use aws.protocols#awsJson1_0

            @awsJson1_0
            service HelloService {
                version: "1"
            }
            """.asSmithyModel()

        clientIntegrationTest(model)
    }
}
