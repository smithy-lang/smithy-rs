/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

/**
 * Regression tests for BDD codegen bugs identified in `.kiro/bdd-codegen-risk-analysis.md`.
 *
 * Bug #1 (`tryGenerateTrivialCondition.visitBoolEquals` does not gate on `ref.isOptional`) is
 * unreachable through valid Smithy models — Smithy's own trait validator rejects
 * `booleanEquals(OptionalBool, literal)` before codegen runs, so no test is included.
 *
 * Bug #3 (`isStringTypedRef` defaulting to `true` on typecheck failure) cannot be triggered
 * through a normal Smithy model because the typechecker runs during model load. The fix makes
 * the fallback conservative regardless; see the comment on `isStringTypedRef` in
 * `BddExpressionGenerator.kt`.
 */
class BddCodegenTest {
    /**
     * Bug #2 regression: `BddExpressionGenerator.isOptionalArgument` previously text-matched a
     * `StringLiteral`'s raw value against parameter names. When a static literal happened to
     * have the same text as an optional parameter (e.g. literal `"Bucket"` while a parameter
     * `Bucket` is also declared), `wrapDefaultArg` wrapped the literal's lowering — `&str` —
     * in `if let Some(param) = ... { param } else { return false }`. That produces invalid
     * Rust because `&str` does not pattern-match `Some(_)`.
     *
     * Fix: `isOptionalSingleDynamicReference` now inspects the [Template] structure via
     * `TemplateVisitor.visitSingleDynamicTemplate` and only flags the argument as optional
     * when it is genuinely a one-element `{Reference}` template wrapping an optional ref.
     *
     * The model below intentionally collides a literal with a parameter name and exercises a
     * library function call (`substring("Bucket", 0, 4, false)`). After the fix, codegen
     * compiles cleanly. Before the fix, it failed to compile with E0308 — see
     * `.kiro/bdd-codegen-test-failures.md`.
     */
    @Test
    fun `string literal whose text matches an optional parameter name is not falsely treated as optional`() {
        val nodes =
            BddTestHelpers.encodeNodes(
                listOf(
                    // condition 0: high → result[1] (the endpoint), low → NoMatch terminal
                    BddTestHelpers.Node(0, BddTestHelpers.resultRef(1), -1),
                ),
            )

        val model =
            """
            namespace test

            use aws.protocols#awsJson1_1
            use smithy.rules#endpointBdd
            use smithy.rules#clientContextParams

            @awsJson1_1
            @clientContextParams(
                Bucket: { type: "string", documentation: "An optional bucket parameter" }
            )
            @endpointBdd({
                version: "1.1",
                "parameters": {
                    "Bucket": {
                        "type": "string",
                        "required": false,
                        "documentation": "An optional bucket parameter"
                    }
                },
                "conditions": [
                    {
                        "fn": "substring",
                        "argv": ["Bucket", 0, 4, false]
                    }
                ],
                "results": [
                    {
                        "conditions": [],
                        "endpoint": {
                            "url": "https://parsed.example.com",
                            "properties": {},
                            "headers": {}
                        },
                        "type": "endpoint"
                    }
                ],
                "root": 2,
                "nodeCount": 2,
                "nodes": "$nodes"
            })
            service TestService {
                version: "1.0",
                operations: [TestOp]
            }

            operation TestOp {
                input: TestInput
            }

            structure TestInput {}
            """.asSmithyModel()

        // Should compile cleanly. Before the fix, this threw CommandError with
        // `mismatched types: expected str, found Option<_>` from the wrap.
        clientIntegrationTest(model)
    }
}
