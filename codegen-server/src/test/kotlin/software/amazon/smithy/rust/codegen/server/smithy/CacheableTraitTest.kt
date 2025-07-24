/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

object CacheableModels {
    fun basicModel(): Model {
        return """
            namespace test

            use smithy.rust#cacheable
            use smithy.protocols#rpcv2Cbor
            use smithy.framework#ValidationException

            @rpcv2Cbor
            service SampleServiceWITHDifferentCASE {
                operations: [SampleOP],
            }
            operation SampleOP {
                input:= {
                    x: String
                }
                output := {
                    @cacheable
                    item: Item

                    cachedItems: CachedItems

                }
            }

            structure Item {
                a: String
            }

            list CachedItems {
                @cacheable
                member: Item
            }
        """.asSmithyModel(smithyVersion = "2")
    }
}

class CacheableTraitTest {
    @Test
    fun `test basic model compiles`() {
        serverIntegrationTest(CacheableModels.basicModel()) { _, _ -> }
    }

    @Test
    fun `test wire caching HTTP response consistency`() {
        serverIntegrationTest(CacheableModels.basicModel()) { _, rustCrate ->
            rustCrate.testModule {
                tokioTest("single_item_http_response_consistency") {
                    rust(
                        """
                        use crate::model::Item;
                        use crate::output::SampleOpOutput;
                        use crate::cacheable::{Cacheable, WireCacheable};
                        use aws_smithy_http_server::response::IntoResponse;
                        use hyper::body::to_bytes;

                        // Create test item
                        let item = Item::builder()
                            .a(Some("test_value".to_string()))
                            .build();

                        // Create output with modeled cacheable
                        let output_modeled = SampleOpOutput::builder()
                            .item(Some(Cacheable::modeled(item.clone())))
                            .build();

                        // Create output with cached cacheable
                        let output_cached = SampleOpOutput::builder()
                            .item(Some(Cacheable::cached(item.to_bytes())))
                            .build();

                        // Convert both to HTTP responses
                        let http_response_modeled = output_modeled.into_response();
                        let http_response_cached = output_cached.into_response();

                        // Extract response bodies
                        let body_modeled = to_bytes(http_response_modeled.into_body()).await
                            .expect("unable to extract body to bytes");
                        let body_cached = to_bytes(http_response_cached.into_body()).await
                            .expect("unable to extract body to bytes");

                        // HTTP response bodies must be byte-for-byte identical
                        assert_eq!(body_modeled, body_cached,
                            "HTTP response bodies must be identical for modeled vs cached items");
                        """,
                    )
                }

                tokioTest("list_http_response_consistency") {
                    rust(
                        """
                        use crate::model::Item;
                        use crate::output::SampleOpOutput;
                        use crate::cacheable::{Cacheable, WireCacheable};
                        use aws_smithy_http_server::response::IntoResponse;
                        use hyper::body::to_bytes;

                        // Create test items
                        let item1 = Item::builder().a(Some("item1".to_string())).build();
                        let item2 = Item::builder().a(Some("item2".to_string())).build();
                        let item3 = Item::builder().a(Some("item3".to_string())).build();

                        // Create outputs with different caching patterns
                        let output_all_modeled = SampleOpOutput::builder()
                            .cached_items(Some(vec![
                                Cacheable::modeled(item1.clone()),
                                Cacheable::modeled(item2.clone()),
                                Cacheable::modeled(item3.clone()),
                            ]))
                            .build();

                        let output_all_cached = SampleOpOutput::builder()
                            .cached_items(Some(vec![
                                Cacheable::cached(item1.to_bytes()),
                                Cacheable::cached(item2.to_bytes()),
                                Cacheable::cached(item3.to_bytes()),
                            ]))
                            .build();

                        let output_mixed = SampleOpOutput::builder()
                            .cached_items(Some(vec![
                                Cacheable::modeled(item1),
                                Cacheable::cached(item2.to_bytes()),
                                Cacheable::modeled(item3),
                            ]))
                            .build();

                        // Convert all to HTTP responses
                        let http_response_modeled = output_all_modeled.into_response();
                        let http_response_cached = output_all_cached.into_response();
                        let http_response_mixed = output_mixed.into_response();

                        // Extract response bodies
                        let body_modeled = to_bytes(http_response_modeled.into_body()).await
                            .expect("unable to extract body to bytes");
                        let body_cached = to_bytes(http_response_cached.into_body()).await
                            .expect("unable to extract body to bytes");
                        let body_mixed = to_bytes(http_response_mixed.into_body()).await
                            .expect("unable to extract body to bytes");

                        // All HTTP response bodies must be byte-for-byte identical
                        assert_eq!(body_modeled, body_cached,
                            "All-modeled vs all-cached HTTP responses must be identical");
                        assert_eq!(body_modeled, body_mixed,
                            "All-modeled vs mixed HTTP responses must be identical");
                        """,
                    )
                }
            }
        }
    }
}
