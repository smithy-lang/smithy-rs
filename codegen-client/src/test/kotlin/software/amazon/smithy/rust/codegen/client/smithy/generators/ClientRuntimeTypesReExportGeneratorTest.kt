/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class ClientRuntimeTypesReExportGeneratorTest {
    private val model =
        """
        namespace test
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service HelloService {
            operations: [],
            version: "1"
        }
        """.asSmithyModel()

    @Test
    fun `it should reexport client runtime types`() {
        clientIntegrationTest(model) { _, crate ->
            crate.unitTest {
                rust(
                    """
                    ##[allow(unused_imports)]
                    {
                        use crate::config::ConfigBag;
                        use crate::config::RuntimeComponents;
                        use crate::config::IdentityCache;

                        use crate::config::endpoint::SharedEndpointResolver;
                        use crate::config::endpoint::EndpointFuture;
                        use crate::config::endpoint::Endpoint;

                        use crate::config::retry::ClassifyRetry;
                        use crate::config::retry::RetryAction;
                        use crate::config::retry::ShouldAttempt;

                        use crate::config::http::HttpRequest;
                        use crate::config::http::HttpResponse;

                        use crate::config::interceptors::AfterDeserializationInterceptorContextRef;
                        use crate::config::interceptors::BeforeDeserializationInterceptorContextMut;
                        use crate::config::interceptors::BeforeDeserializationInterceptorContextRef;
                        use crate::config::interceptors::BeforeSerializationInterceptorContextMut;
                        use crate::config::interceptors::BeforeSerializationInterceptorContextRef;
                        use crate::config::interceptors::BeforeTransmitInterceptorContextMut;
                        use crate::config::interceptors::BeforeTransmitInterceptorContextRef;
                        use crate::config::interceptors::FinalizerInterceptorContextMut;
                        use crate::config::interceptors::FinalizerInterceptorContextRef;
                        use crate::config::interceptors::InterceptorContext;

                        use crate::error::BoxError;
                    }
                    """,
                )
            }
        }
    }
}
