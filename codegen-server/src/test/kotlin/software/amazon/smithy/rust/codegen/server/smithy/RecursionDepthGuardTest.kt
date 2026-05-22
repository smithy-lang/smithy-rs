/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * End-to-end regression test for deeply-nested payload returning a deserialization
 * error rather than overflowing the thread stack.
 *
 * The model below contains a `TreeService` with a `PutTree` operation whose input is the recursive
 * `Node` / `NodeList` pair — the canonical pattern that used to crash the server
 * process with SIGABRT on a ~100 KB payload.
 *
 * The test drives the generated `PutTreeInput::from_request` handler on a thread
 * with a deliberately small (1 MiB) stack. That stack is:
 *  - Small enough that the unpatched code would overflow it (and therefore
 *    abort the whole process, failing the test) well before the payload is
 *    exhausted.
 *  - Large enough that the patched code returns `Err(...)` with
 *    the "maximum nesting depth exceeded" message after 128 recursive descents,
 *    long before running out of stack.
 *
 * Running the deserializer on a restricted-stack thread keeps the regression
 * detectable on CI hosts that have generous default stacks (Linux typically
 * grants 8 MiB), where the unpatched bug would manifest only at much greater
 * depths.
 */
internal class RecursionDepthGuardTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace recursionTest

        use aws.protocols#restJson1
        use smithy.framework#ValidationException

        @restJson1
        service TreeService {
            version: "2026-05-01",
            operations: [PutTree]
        }

        @http(method: "POST", uri: "/put-tree")
        operation PutTree {
            input := { root: Node }
            output := { ok: Boolean }
            errors: [ValidationException]
        }

        structure Node {
            name: String,
            children: NodeList
        }

        list NodeList {
            member: Node
        }
        """.asSmithyModel()

    @Test
    fun `deeply nested recursive JSON payload returns an error instead of overflowing the stack`() {
        serverIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                unitTest("deep_recursive_payload_is_rejected_with_nesting_depth_error") {
                    rustTemplate(
                        """
                        use #{SmithyHttpServer}::request::FromRequest;

                        // Build a JSON payload 200 levels deep:
                        //   {"root":{"children":[{"children":[ ... {"children":[]} ... ]}]}}
                        // Each level adds 2 to the parser's shape-tree depth counter
                        // (one for the NodeList, one for the Node element), so a depth-128
                        // guard fires at roughly level 64 — well short of the bottom.
                        const DEPTH: usize = 200;
                        let mut payload = String::from(r##"{"root":"##);
                        for _ in 0..DEPTH {
                            payload.push_str(r##"{"children":["##);
                        }
                        // Innermost Node has an empty children list.
                        payload.push_str(r##"{"children":[]}"##);
                        for _ in 0..DEPTH {
                            payload.push_str("]}");
                        }
                        payload.push('}');
                        let payload_bytes = #{Bytes}::from(payload.into_bytes());

                        // Run the deserializer on a thread with a 1 MiB stack. If the
                        // recursion guard is missing or broken, the thread overflows its
                        // guard page and the whole process aborts — failing the test.
                        // With the guard in place, deserialization returns an `Err` well
                        // before running out of stack.
                        let handle = ::std::thread::Builder::new()
                            .stack_size(1 << 20) // 1 MiB
                            .spawn(move || {
                                let rt = ::tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                    .expect("failed to build tokio runtime");
                                rt.block_on(async move {
                                    let request = #{Http}::Request::builder()
                                        .uri("/put-tree")
                                        .method("POST")
                                        .header("Content-Type", "application/json")
                                        .body(#{Body:W})
                                        .expect("failed to build request");
                                    crate::input::PutTreeInput::from_request(request)
                                        .await
                                        .map(|_| ())
                                        .map_err(|e| format!("{e}"))
                                })
                            })
                            .expect("failed to spawn bounded-stack thread");

                        let result = handle
                            .join()
                            .expect("deserializer thread panicked or aborted — likely a stack overflow, which means the recursion-depth guard is not firing");

                        let err = result.expect_err(
                            "deeply-nested recursive payload should have been rejected by the recursion-depth guard, but deserialization succeeded",
                        );
                        assert!(
                            err.contains("maximum nesting depth exceeded"),
                            "expected the deserialization error to mention the nesting-depth guard, got: {err}",
                        );
                        """,
                        "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                        "Bytes" to RuntimeType.Bytes,
                        "Body" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "payload_bytes"),
                    )
                }
            }
        }
    }
}
