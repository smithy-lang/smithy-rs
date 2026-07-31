/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.awsSdkIntegrationTest

/**
 * End-to-end verification of invalidation-on-auth-failure (F-INVAL-1): a generated client whose
 * default identity cache is `StaticStabilityCache` (installed by the codegen decorator) must, on an
 * `ExpiredToken` service rejection, invalidate the cached identity so the next call re-resolves
 * rather than reusing the rejected credentials.
 *
 * Exercises the whole loop with no `aws-config` and no real service: per-op
 * `CredentialAuthFailureInterceptor` (codegen) -> `InvalidateResolvedIdentity` marker ->
 * orchestrator bridge -> `StaticStabilityCache::invalidate`. Reuses [SdkCodegenIntegrationTest.model]
 * and its `NeatOperation` (which requires SigV4 auth, so it resolves credentials).
 */
class CredentialAuthFailureInterceptorTest {
    @Test
    fun expiredTokenInvalidatesCachedIdentity() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, rustCrate ->
            val moduleUseName = ctx.moduleUseName()
            val rc = ctx.runtimeConfig
            rustCrate.integrationTest("credential_auth_failure_invalidation") {
                Attribute.featureGate("test-util").render(this)
                tokioTest("expired_token_invalidates_cached_identity") {
                    rustTemplate(
                        """
                        use std::sync::atomic::{AtomicUsize, Ordering};
                        use std::sync::Arc;

                        // Credential provider that counts how many times it is resolved.
                        ##[derive(Clone, Debug)]
                        struct Counting {
                            n: Arc<AtomicUsize>,
                        }
                        impl #{ProvideCredentials} for Counting {
                            fn provide_credentials<'a>(&'a self) -> #{Fut}<'a>
                            where
                                Self: 'a,
                            {
                                self.n.fetch_add(1, Ordering::SeqCst);
                                #{Fut}::ready(Ok(#{Credentials}::for_tests()))
                            }
                        }

                        // Reject the first request with an ExpiredToken error; succeed afterwards.
                        let request_count = Arc::new(AtomicUsize::new(0));
                        let http_client = {
                            let request_count = request_count.clone();
                            #{infallible_client_fn}(move |_req| {
                                if request_count.fetch_add(1, Ordering::SeqCst) == 0 {
                                    #{Response}::builder()
                                        .status(400)
                                        .header("x-amzn-errortype", "ExpiredToken")
                                        .body(#{SdkBody}::from("{}"))
                                        .unwrap()
                                } else {
                                    #{Response}::builder()
                                        .status(200)
                                        .body(#{SdkBody}::from("{}"))
                                        .unwrap()
                                }
                            })
                        };

                        let creds = Counting {
                            n: Arc::new(AtomicUsize::new(0)),
                        };
                        let config = $moduleUseName::Config::builder()
                            .http_client(http_client)
                            .region(#{Region}::new("us-east-1"))
                            .credentials_provider(creds.clone())
                            .behavior_version_latest()
                            .build();
                        let client = $moduleUseName::Client::from_conf(config);

                        // Call 1: identity resolved + cached (n == 1), signed, rejected with ExpiredToken.
                        // The per-op CredentialAuthFailureInterceptor sets the invalidate marker; the
                        // orchestrator invalidates the cached identity.
                        let _ = client.neat_operation().send().await;

                        // Call 2: the cached identity was invalidated, so the cache re-resolves instead
                        // of reusing the (rejected) credentials (n == 2).
                        let _ = client.neat_operation().send().await;

                        assert_eq!(
                            2,
                            creds.n.load(Ordering::SeqCst),
                            "ExpiredToken must invalidate the cached identity, forcing a re-resolve"
                        );
                        """,
                        "ProvideCredentials" to
                            AwsRuntimeType.awsCredentialTypes(rc).resolve("provider::ProvideCredentials"),
                        "Fut" to
                            AwsRuntimeType.awsCredentialTypes(rc).resolve("provider::future::ProvideCredentials"),
                        "Credentials" to
                            AwsRuntimeType.awsCredentialTypesTestUtil(rc).resolve("Credentials"),
                        "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(rc).toType()
                                .resolve("test_util::infallible_client_fn"),
                        "Response" to RuntimeType.HttpResponse1x,
                        "SdkBody" to RuntimeType.sdkBody(rc),
                    )
                }
            }
        }
    }
}
