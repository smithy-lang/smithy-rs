/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CodegenVisitor
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.testutil.TokioTest
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.util.runCommand

// If any of these tests fail, and you want to understand why, run them with logging:
// ```
// ./gradlew codegen:test --tests software.amazon.smithy.rust.codegen.customizations.HttpVersionListGeneratorTest --info
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
        val (ctx, testDir) = generatePluginContext(model)
        val moduleName = ctx.settings.expectStringMember("module").value.replace('-', '_')
        val testWriter = object : RustCodegenDecorator<ClientCodegenContext> {
            override val name: String = "add tests"
            override val order: Byte = 0

            override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
                rustCrate.withFile("tests/validate_defaults.rs") {
                    TokioTest.render(it)
                    it.rust(
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
                        """
                    )
                }
            }
        }
        val combinedCodegenDecorator: CombinedCodegenDecorator<ClientCodegenContext> =
            CombinedCodegenDecorator.fromClasspath(ctx, RequiredCustomizations()).withDecorator(testWriter)
        val visitor = CodegenVisitor(ctx, combinedCodegenDecorator)
        visitor.execute()
        "cargo test".runCommand(testDir)
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
        val (ctx, testDir) = generatePluginContext(model)
        val moduleName = ctx.settings.expectStringMember("module").value.replace('-', '_')
        val testWriter = object : RustCodegenDecorator<ClientCodegenContext> {
            override val name: String = "add tests"
            override val order: Byte = 0

            override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
                rustCrate.withFile("tests/validate_http.rs") {
                    TokioTest.render(it)
                    it.rust(
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
                        """
                    )
                }
            }
        }

        val combinedCodegenDecorator: CombinedCodegenDecorator<ClientCodegenContext> =
            CombinedCodegenDecorator.fromClasspath(ctx, RequiredCustomizations()).withDecorator(testWriter)
        val visitor = CodegenVisitor(ctx, combinedCodegenDecorator)
        visitor.execute()
        "cargo test".runCommand(testDir)
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

        val (ctx, testDir) = generatePluginContext(model, addModuleToEventStreamAllowList = true)
        val moduleName = ctx.settings.expectStringMember("module").value.replace('-', '_')
        val codegenDecorator = object : RustCodegenDecorator<ClientCodegenContext> {
            override val name: String = "add tests"
            override val order: Byte = 0

            override fun configCustomizations(
                codegenContext: ClientCodegenContext,
                baseCustomizations: List<ConfigCustomization>
            ): List<ConfigCustomization> {
                return super.configCustomizations(codegenContext, baseCustomizations) + FakeSigningConfig(codegenContext.runtimeConfig)
            }

            override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
                rustCrate.withFile("tests/validate_eventstream_http.rs") {
                    TokioTest.render(it)
                    it.rust(
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
                        """
                    )
                }
            }
        }

        val combinedCodegenDecorator: CombinedCodegenDecorator<ClientCodegenContext> =
            CombinedCodegenDecorator.fromClasspath(ctx, RequiredCustomizations()).withDecorator(codegenDecorator)
        val visitor = CodegenVisitor(ctx, combinedCodegenDecorator)
        visitor.execute()
        "cargo test".runCommand(testDir)
    }
}

class FakeSigningConfig(
    runtimeConfig: RuntimeConfig,
) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "SharedPropertyBag" to RuntimeType(
            "SharedPropertyBag",
            CargoDependency.SmithyHttp(runtimeConfig),
            "aws_smithy_http::property_bag"
        ),
        "SignMessageError" to RuntimeType(
            "SignMessageError",
            CargoDependency.SmithyEventStream(runtimeConfig),
            "aws_smithy_eventstream::frame"
        ),
        "SignMessage" to RuntimeType(
            "SignMessage",
            CargoDependency.SmithyEventStream(runtimeConfig),
            "aws_smithy_eventstream::frame"
        ),
        "Message" to RuntimeType(
            "Message",
            CargoDependency.SmithyEventStream(runtimeConfig),
            "aws_smithy_eventstream::frame"
        )
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
                    *codegenScope
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

                        fn sign_empty(&mut self) -> Result<#{Message}, #{SignMessageError}> {
                            Ok(#{Message}::new(Vec::new()))
                        }
                    }
                    """,
                    *codegenScope
                )
            }
            else -> emptySection
        }
    }
}
