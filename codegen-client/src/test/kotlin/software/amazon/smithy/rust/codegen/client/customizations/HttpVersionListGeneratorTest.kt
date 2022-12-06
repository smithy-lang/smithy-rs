/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.EventStreamSigningConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TokioTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

// If any of these tests fail, and you want to understand why, run them with logging:
// ```
// ./gradlew codegen-client:test --tests software.amazon.smithy.rust.codegen.client.HttpVersionListGeneratorTest --info
// ```

internal class HttpVersionListGeneratorTest {
    @Test
    fun `http version list integration test (no preferred version)`() {
        val model = """
            namespace com.example

            use aws.protocols#awsJson1_0

            @awsJson1_0
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentPrefix")
            service TestService {
                operations: [SayHello],
                version: "1"
            }

            @endpoint(hostPrefix: "test123.{greeting}.")
            operation SayHello {
                input: SayHelloInput
            }

            structure SayHelloInput {
                @required
                @hostLabel
                greeting: String
            }
        """.asSmithyModel()
        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("http_version_list") {
                TokioTest.render(this)
                rust(
                    """
                    async fn test_http_version_list_defaults() {
                        let conf = $moduleName::Config::builder().build();
                        let op = $moduleName::operation::SayHello::builder()
                            .greeting("hello")
                            .build().expect("valid operation")
                            .make_operation(&conf).await.expect("hello is a valid prefix");
                        let properties = op.properties();
                        let actual_http_versions = properties.get::<Vec<http::Version>>()
                            .expect("http versions list should be in property bag");
                        let expected_http_versions = &vec![http::Version::HTTP_11];
                        assert_eq!(actual_http_versions, expected_http_versions);
                    }
                    """,
                )
            }
        }
    }

    @Test
    fun `http version list integration test (http)`() {
        val model = """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1(http: ["http/1.1", "h2"])
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentPrefix")
            service TestService {
                operations: [SayHello],
                version: "1"
            }

            @http(method: "PUT", uri: "/input", code: 200)
            @idempotent
            @endpoint(hostPrefix: "test123.{greeting}.")
            operation SayHello {
                input: SayHelloInput
            }

            structure SayHelloInput {
                @required
                @hostLabel
                greeting: String
            }
        """.asSmithyModel()
        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("validate_http") {
                TokioTest.render(this)
                rust(
                    """
                    async fn test_http_version_list_defaults() {
                        let conf = $moduleName::Config::builder().build();
                        let op = $moduleName::operation::SayHello::builder()
                            .greeting("hello")
                            .build().expect("valid operation")
                            .make_operation(&conf).await.expect("hello is a valid prefix");
                        let properties = op.properties();
                        let actual_http_versions = properties.get::<Vec<http::Version>>()
                            .expect("http versions list should be in property bag");
                        let expected_http_versions = &vec![http::Version::HTTP_11, http::Version::HTTP_2];
                        assert_eq!(actual_http_versions, expected_http_versions);
                    }
                    """,
                )
            }
        }
    }

    @Test
    fun `http version list integration test (eventStreamHttp)`() {
        val model = """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1(http: ["h2", "http/1.1"], eventStreamHttp: ["h2"])
            @aws.api#service(sdkId: "Test")
            service TestService {
                operations: [SayHello],
                version: "1"
            }

            @idempotent
            @http(uri: "/test", method: "PUT")
            operation SayHello {
                input: SayHelloInput,
                output: SayHelloOutput
            }

            structure SayHelloInput {}

            structure SayHelloOutput {
                @httpPayload
                greeting: SomeStream
            }

            structure Something { stuff: SomethingElse }

            structure SomethingElse {
                version: String
            }

            @streaming
            union SomeStream {
                Something: Something,
            }
        """.asSmithyModel()

        clientIntegrationTest(
            model,
            listOf(FakeSigningDecorator()),
            addModuleToEventStreamAllowList = true,
        ) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("validate_eventstream_http") {
                TokioTest.render(this)
                rust(
                    """
                    async fn test_http_version_list_defaults() {
                        let conf = $moduleName::Config::builder().build();
                        let op = $moduleName::operation::SayHello::builder()
                            .build().expect("valid operation")
                            .make_operation(&conf).await.unwrap();
                        let properties = op.properties();
                        let actual_http_versions = properties.get::<Vec<http::Version>>()
                            .expect("http versions list should be in property bag");
                        let expected_http_versions = &vec![http::Version::HTTP_2];
                        assert_eq!(actual_http_versions, expected_http_versions);
                    }
                    """,
                )
            }
        }
    }
}

class FakeSigningDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "fakesigning"
    override val order: Byte = 0
    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean = false
    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.filterNot { it is EventStreamSigningConfig } + FakeSigningConfig(codegenContext.runtimeConfig)
    }
}

class FakeSigningConfig(
    runtimeConfig: RuntimeConfig,
) : EventStreamSigningConfig(runtimeConfig) {
    private val codegenScope = arrayOf(
        "SharedPropertyBag" to RuntimeType.smithyHttp(runtimeConfig).resolve("property_bag::SharedPropertyBag"),
        "SignMessageError" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::SignMessageError"),
        "SignMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::SignMessage"),
        "Message" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::Message"),
    )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigImpl -> writable {
                rustTemplate(
                    """
                    /// Creates a new Event Stream `SignMessage` implementor.
                    pub fn new_event_stream_signer(
                        &self,
                        properties: #{SharedPropertyBag}
                    ) -> FakeSigner {
                        FakeSigner::new(properties)
                    }
                    """,
                    *codegenScope,
                )
            }

            is ServiceConfig.Extras -> writable {
                rustTemplate(
                    """
                    /// Fake signing implementation.
                    ##[derive(Debug)]
                    pub struct FakeSigner;

                    impl FakeSigner {
                        /// Create a real `FakeSigner`
                        pub fn new(_properties: #{SharedPropertyBag}) -> Self {
                            Self {}
                        }
                    }

                    impl #{SignMessage} for FakeSigner {
                        fn sign(&mut self, message: #{Message}) -> Result<#{Message}, #{SignMessageError}> {
                            Ok(message)
                        }

                        fn sign_empty(&mut self) -> Option<Result<#{Message}, #{SignMessageError}>> {
                            Some(Ok(#{Message}::new(Vec::new())))
                        }
                    }
                    """,
                    *codegenScope,
                )
            }

            else -> emptySection
        }
    }
}
