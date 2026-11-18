/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class SigV4EventStreamSupportStructuresTest {
    private val runtimeConfig = TestRuntimeConfig

    @Test
    fun `support structures compile`() {
        val project = TestWorkspace.testProject()
        project.lib {
            val codegenScope = SigV4EventStreamSupportStructures.codegenScope(runtimeConfig)

            // Generate the support structures - RuntimeType.forInlineFun automatically generates the code
            // when the RuntimeType is used, so we just need to reference them
            rustTemplate(
                """
                use std::time::SystemTime;

                // Reference the types to trigger their generation
                fn _test_types() {
                    let _info: #{SignatureInfo};
                    let _error: #{ExtractionError};
                    let _signed_error: #{SignedEventError}<String>;
                    let _signed_event: #{SignedEvent}<String>;
                    let _unmarshaller: #{SigV4Unmarshaller}<String>;
                }
                """,
                *codegenScope,
            )

            unitTest("test_signature_info_creation") {
                rustTemplate(
                    """
                    let info = #{SignatureInfo} {
                        chunk_signature: vec![1, 2, 3],
                        timestamp: SystemTime::now(),
                    };
                    assert_eq!(info.chunk_signature, vec![1, 2, 3]);
                    """,
                    *codegenScope,
                )
            }

            unitTest("test_signed_event_creation") {
                rustTemplate(
                    """
                    let event = #{SignedEvent} {
                        message: "test".to_string(),
                        signature: None,
                    };
                    assert_eq!(event.message, "test");
                    assert!(event.signature.is_none());
                    """,
                    *codegenScope,
                )
            }

            unitTest("test_signed_event_error_preserves_sources") {
                rustTemplate(
                    """
                    let event_error = #{SignedEventError}::Event(#{IoError}::other("event failed"));
                    assert_eq!(event_error.to_string(), "event failed");
                    let source = #{Error}::source(&event_error).unwrap();
                    assert_eq!(source.downcast_ref::<#{IoError}>().unwrap().to_string(), "event failed");

                    let extraction_error = #{SignedEventError}::<#{IoError}>::InvalidSignedEvent(
                        #{ExtractionError}::InvalidTimestamp,
                    );
                    assert_eq!(extraction_error.to_string(), "invalid or missing timestamp header");
                    let source = #{Error}::source(&extraction_error).unwrap();
                    assert!(source.downcast_ref::<#{ExtractionError}>().is_some());
                    """,
                    "IoError" to RuntimeType.std.resolve("io::Error"),
                    "Error" to RuntimeType.StdError,
                    *codegenScope,
                )
            }
        }

        project.compileAndTest()
    }
}
