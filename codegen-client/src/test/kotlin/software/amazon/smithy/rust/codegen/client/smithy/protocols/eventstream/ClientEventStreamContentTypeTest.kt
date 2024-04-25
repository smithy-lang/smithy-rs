/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq

class ClientEventStreamContentTypeTest {
    @Test
    fun `restJson event stream message content type`() {
        test(EventStreamTestModels.restJson1(), "application/json")
    }

    @Test
    fun `restXml event stream message content type`() {
        test(EventStreamTestModels.restXml(), "application/xml")
    }

    @Test
    fun `awsJson event stream message content type`() {
        test(EventStreamTestModels.awsJson11(), "application/json")
    }

    private fun test(
        model: Model,
        messageContentType: String,
    ) {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.lib {
                rust(
                    """
                    ##[cfg(test)]
                    fn test_content_type(input: crate::types::TestStream) -> String {
                        use aws_smithy_eventstream::frame::MarshallMessage;

                        let marshaller = crate::event_stream_serde::TestStreamMarshaller::new();
                        let marshalled = marshaller.marshall(input).expect("success");
                        marshalled
                            .headers()
                            .iter()
                            .find(|h| h.name().as_str() == ":content-type")
                            .expect("has a :content-type header")
                            .value()
                            .as_string()
                            .expect(":content-type is a string")
                            .as_str()
                            .into()
                    }
                    """,
                )
            }
            rustCrate.unitTest("blob_content_type") {
                rust(
                    """
                    let input = crate::types::TestStream::MessageWithBlob(
                        crate::types::MessageWithBlob::builder()
                            .data(crate::primitives::Blob::new(b"blob"))
                            .build()
                    );
                    assert_eq!("application/octet-stream", test_content_type(input));
                    """,
                )
            }
            rustCrate.unitTest("string_content_type") {
                rust(
                    """
                    let input = crate::types::TestStream::MessageWithString(
                        crate::types::MessageWithString::builder()
                            .data("foobaz")
                            .build()
                    );
                    assert_eq!("text/plain", test_content_type(input));
                    """,
                )
            }
            rustCrate.unitTest("struct_content_type") {
                rust(
                    """
                    let input = crate::types::TestStream::MessageWithStruct(
                        crate::types::MessageWithStruct::builder()
                            .some_struct(crate::types::TestStruct::builder().some_string("foo").build())
                            .build()
                    );
                    assert_eq!(${messageContentType.dq()}, test_content_type(input));
                    """,
                )
            }
        }
    }
}
