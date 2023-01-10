/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.endpoints

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.traits.EndpointTestCase
import software.amazon.smithy.rulesengine.traits.EndpointTestOperationInput
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.clientInstantiator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.nonEmptyOrNull
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import java.util.logging.Logger

class OperationInputTestDecorator : ClientCodegenDecorator {
    override val name: String = "OperationInputTest"
    override val order: Byte = 0

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val endpointTests = EndpointTypesGenerator.fromContext(codegenContext).tests.nonEmptyOrNull() ?: return
        rustCrate.integrationTest("endpoint_tests") {
            val tests = endpointTests.flatMap { test ->
                val generator = OperationInputTestGenerator(codegenContext, test)
                test.operationInputs.filterNot { usesUnsupportedBuiltIns(it) }.map { operationInput ->
                    generator.generateInput(operationInput)
                }
            }
            tests.join("\n")(this)
        }
    }
}

private val deprecatedBuiltins =
    setOf(Builtins.S3_USE_GLOBAL_ENDPOINT, Builtins.STS_USE_GLOBAL_ENDPOINT).map { it.builtIn.get() }

fun usesUnsupportedBuiltIns(testOperationInput: EndpointTestOperationInput): Boolean {
    return testOperationInput.builtInParams.members.map { it.key.value }.any { deprecatedBuiltins.contains(it) }
}

/**
 * Eventually, we need to pull this test into generic smithy. However, this relies on generic smithy clients
 * supporting middleware and being instantiable from config (https://github.com/awslabs/smithy-rs/issues/2194)
 *
 * Doing this in AWS codegen allows us to actually integration test generated clients.
 */
class OperationInputTestGenerator(private val ctx: ClientCodegenContext, private val test: EndpointTestCase) {
    private val runtimeConfig = ctx.runtimeConfig
    private val moduleName = ctx.moduleUseName()
    private val endpointCustomizations = ctx.rootDecorator.endpointCustomizations(ctx)
    private val model = ctx.model
    val symbolProvider = ctx.symbolProvider
    private val instantiator = clientInstantiator(ctx)

    private fun EndpointTestOperationInput.operationId() =
        ShapeId.fromOptionalNamespace(ctx.serviceShape.id.namespace, operationName)

    fun generateInput(testOperationInput: EndpointTestOperationInput) = writable {
        val operationName = testOperationInput.operationName.toSnakeCase()
        tokioTest(safeName("operation_input_test_$operationName")) {
            rustTemplate(
                """
                /* builtIns: ${Node.prettyPrintJson(testOperationInput.builtInParams)} */
                /* clientParams: ${Node.prettyPrintJson(testOperationInput.clientParams)} */
                let (conn, rcvr) = #{capture_request}(None);
                let conf = #{conf};
                let client = $moduleName::Client::from_conf(conf);
                let _result = dbg!(#{invoke_operation});
                #{assertion}
                """,
                "capture_request" to CargoDependency.smithyClient(runtimeConfig)
                    .withFeature("test-util").toType().resolve("test_connection::capture_request"),
                "conf" to config(testOperationInput),
                "invoke_operation" to operationInvocation(testOperationInput),
                "assertion" to writable {
                    test.expect.endpoint.ifPresent { endpoint ->
                        val uri = escape(endpoint.url)
                        rustTemplate(
                            """
                            let req = rcvr.expect_request();
                            let uri = req.uri().to_string();
                            assert!(uri.starts_with(${uri.dq()}), "expected URI to start with `$uri` but it was `{}`", uri);
                            """,
                        )
                    }
                    test.expect.error.ifPresent { error ->
                        val expectedError =
                            escape("expected error: $error [${test.documentation.orNull() ?: "no docs"}]")
                        rustTemplate(
                            """
                            let error = _result.expect_err(${expectedError.dq()});
                            assert!(format!("{:?}", error).contains(${escape(error).dq()}), "expected error to contain `${
                            escape(
                                error,
                            )
                            }` but it was {}", format!("{:?}", error));
                            """,
                        )
                    }
                },
            )
        }
    }

    private fun operationInvocation(testOperationInput: EndpointTestOperationInput) = writable {
        rust("client.${testOperationInput.operationName.toSnakeCase()}()")
        val operationInput =
            model.expectShape(testOperationInput.operationId(), OperationShape::class.java).inputShape(model)
        testOperationInput.operationParams.members.forEach { (key, value) ->
            val member = operationInput.expectMember(key.value)
            rustTemplate(
                ".${member.setterName()}(#{value})",
                "value" to instantiator.generate(member, value),
            )
        }
        rust(".send().await")
    }

    /** initialize service config for test */
    private fun config(operationInput: EndpointTestOperationInput) = writable {
        rustBlock("") {
            Attribute.AllowUnusedMut.render(this)
            rust("let mut builder = $moduleName::Config::builder().with_test_defaults().http_connector(conn);")
            operationInput.builtInParams.members.forEach { (builtIn, value) ->
                val setter = endpointCustomizations.firstNotNullOfOrNull {
                    it.setBuiltInOnConfig(
                        builtIn.value,
                        value,
                        "builder",
                    )
                }
                if (setter != null) {
                    setter(this)
                } else {
                    Logger.getLogger("OperationTestGenerator").warning("No provider for ${builtIn.value}")
                }
            }
            rust("builder.build()")
        }
    }
}
