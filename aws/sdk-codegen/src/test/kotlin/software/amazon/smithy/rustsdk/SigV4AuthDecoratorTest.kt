/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenVisitor
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ClientCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpConnectorConfigDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.IdempotencyTokenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SensitiveOutputDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.StaticSdkFeatureTrackerDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointParamsDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.StalledStreamProtectionDecorator
import software.amazon.smithy.rust.codegen.client.testutil.ClientDecoratableBuildPlugin
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class SigV4AuthDecoratorTest {
    private val modelWithSigV4AuthScheme =
        """
        namespace test

        use aws.auth#sigv4
        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use aws.auth#unsignedPayload
        use smithy.test#httpRequestTests

        @auth([sigv4])
        @sigv4(name: "dontcare")
        @restJson1
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
            "parameters": {
                "endpoint": { "required": true, "type": "string", "builtIn": "SDK::Endpoint" },
            }
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }
        structure SomeOutput { something: String }

        structure SomeInput {
            @httpPayload
            @required
            something: Bytestream
         }

        @streaming
        blob Bytestream

        @httpRequestTests([{
            id: "unsignedPayload",
            protocol: restJson1,
            method: "POST",
            uri: "/",
            params: {
                something: "hello"
            },
            headers: {
                "x-amz-content-sha256": "UNSIGNED-PAYLOAD",
            },
        }])
        @unsignedPayload
        @http(uri: "/", method: "POST")
        operation SomeOperation { input: SomeInput, output: SomeOutput }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun unsignedPayloadSetsCorrectHeader() {
        awsSdkIntegrationTest(modelWithSigV4AuthScheme) { _, _ -> }
    }

    private val modelWithSigV4aAuthScheme =
        """
        namespace test

        use aws.auth#sigv4
        use aws.auth#sigv4a
        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use aws.auth#unsignedPayload
        use smithy.test#httpRequestTests

        @auth([sigv4a,sigv4])
        @sigv4(name: "dontcare")
        @sigv4a(name: "dontcare")
        @restJson1
        @endpointRuleSet({
            "version": "1.0",
            "rules": [
                {
                    "type": "endpoint",
                    "conditions": [],
                    "endpoint": {
                        "url": "https://example.com",
                        "properties": {
                            "authSchemes": [
                                {
                                    "name": "sigv4a",
                                    "signingRegionSet": ["*"],
                                    "signingName": "dontcare"
                                }
                            ]
                        }
                    }
                }
            ],
            "parameters": {
                "endpoint": { "required": true, "type": "string", "builtIn": "SDK::Endpoint" },
            }
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }

        @streaming
        blob Bytestream

        structure SomeInput {
            @httpPayload
            @required
            something: Bytestream
         }

        structure SomeOutput { something: String }

        @http(uri: "/", method: "POST")
        operation SomeOperation { input: SomeInput, output: SomeOutput }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun unsignedPayloadSetsCorrectHeaderForSigV4a() {
        awsSdkIntegrationTest(
            modelWithSigV4aAuthScheme,
            // TODO(IdentityAndAuth): Remove buildPlugin parameter override once codegen respects the order of auth
            //  trait entries in a model
            buildPlugin = ForceEndpointBaseAuthRustClientCodegenPlugin(),
        ) { clientCodegenContext, rustCrate ->
            val moduleUseName = clientCodegenContext.moduleUseName()
            val rc = clientCodegenContext.runtimeConfig

            rustCrate.integrationTest("sigv4a") {
                Attribute.featureGate("test-util").render(this)
                tokioTest("test_sigv4a_signing") {
                    rustTemplate(
                        """
                        let http_client = #{StaticReplayClient}::new(vec![#{ReplayEvent}::new(
                            #{Request}::builder()
                                .header("authorization", "AWS4-ECDSA-P256-SHA256 Credential=ANOTREAL/20090213/dontcare/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-date;x-amz-region-set;x-amz-user-agent, Signature=3045022100b95d1c054ff04b676d12f0c893348606844d67ccf595981f0ca4968fae2eddfd022073e66edc0ad1da05b08392fccefa3ad69f8ec9393461033412fa05c55b749e9d")
                                .uri("https://example.com")
                                .body(#{SdkBody}::from("Hello, world!"))
                                .unwrap(),
                            #{Response}::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                        )]);
                        let config = $moduleUseName::Config::builder()
                            .http_client(http_client.clone())
                            .endpoint_url("https://example.com")
                            .behavior_version_latest()
                            .with_test_defaults()
                            .build();
                        let client = $moduleUseName::Client::from_conf(config);
                        let _ = client.some_operation().something(#{ByteStream}::from_static(b"Hello, world!")).send().await;

                        http_client.assert_requests_match(&["authorization"]);
                        let auth_header = http_client.actual_requests().next().unwrap().headers().get(http::header::AUTHORIZATION).unwrap();
                        assert!(auth_header.contains("AWS4-ECDSA-P256-SHA256"));
                        """,
                        "ByteStream" to RuntimeType.byteStream(rc),
                        "Credentials" to AwsRuntimeType.awsCredentialTypesTestUtil(rc).resolve("Credentials"),
                        "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                        "ReplayEvent" to CargoDependency.smithyHttpClientTestUtil(rc).toType().resolve("test_util::ReplayEvent"),
                        "Request" to RuntimeType.HttpRequest,
                        "Response" to RuntimeType.HttpResponse,
                        "SdkBody" to RuntimeType.sdkBody(rc),
                        "StaticReplayClient" to CargoDependency.smithyHttpClientTestUtil(rc).toType().resolve("test_util::StaticReplayClient"),
                        "tracing_subscriber" to RuntimeType.TracingSubscriber,
                    )
                }
            }
        }
    }
}

// TODO(IdentityAndAuth): Remove this class once codegen respects the order of auth trait entries in a model
// This is a workaround to use `EndpointBasedAuthSchemeOptionDecorator` is during testing, as it is allow-listed
// based on services in production code.
// Without `EndpointBasedAuthSchemeOptionDecorator`, the `unsignedPayloadSetsCorrectHeaderForSigV4a` test would
// fail because `StaticAuthSchemeOptionResolver` would be used instead. The codegen does NOT pass to it auth trait
// entries in the order specified in `modelWithSigV4aAuthScheme` and instead passes sigv4 first and sigv4a in a list.
private class ForceEndpointBaseAuthRustClientCodegenPlugin : ClientDecoratableBuildPlugin() {
    override fun getName(): String = "force-endpoint-based-auth-rust-client-codegen"

    override fun executeWithDecorator(
        context: PluginContext,
        vararg decorator: ClientCodegenDecorator,
    ) {
        // Same combined decorators in `RustClientCodegenPlugin`, used by `awsSdkIntegrationTest`,
        // plus a decorator to enable `EndpointBasedAuthSchemeOption`
        val codegenDecorator =
            CombinedClientCodegenDecorator.fromClasspath(
                context,
                ClientCustomizations(),
                RequiredCustomizations(),
                FluentClientDecorator(),
                EndpointsDecorator(),
                EndpointParamsDecorator(),
                NoAuthDecorator(),
                HttpAuthDecorator(),
                HttpConnectorConfigDecorator(),
                SensitiveOutputDecorator(),
                IdempotencyTokenDecorator(),
                StalledStreamProtectionDecorator(),
                StaticSdkFeatureTrackerDecorator(),
                object : ClientCodegenDecorator {
                    override val name: String get() = "ForceEndpointBasedAuthSchemeOptionDecorator"
                    override val order: Byte = 0

                    override fun authOptions(
                        codegenContext: ClientCodegenContext,
                        operationShape: OperationShape,
                        baseAuthSchemeOptions: List<AuthSchemeOption>,
                    ): List<AuthSchemeOption> =
                        baseAuthSchemeOptions +
                            AuthSchemeOption.EndpointBasedAuthSchemeOption
                },
                *decorator,
            )

        ClientCodegenVisitor(context, codegenDecorator).execute()
    }
}
