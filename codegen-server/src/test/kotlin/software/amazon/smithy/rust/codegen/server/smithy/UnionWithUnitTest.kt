/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class UnionWithUnitTest {
    @Test
    fun `a constrained union that has a unit member should compile`() {
        val model =
            """
            ${'$'}version: "2"
            namespace com.example
            use aws.protocols#restJson1
            use smithy.framework#ValidationException
            
            @restJson1 @title("Test Service") 
            service TestService { 
                version: "0.1", 
                operations: [ 
                    TestOperation
                    TestSimpleUnionWithUnit
                ] 
            }
            
            @http(uri: "/testunit", method: "POST")
            operation TestSimpleUnionWithUnit {
                input := {
                    @required
                    request: SomeUnionWithUnit
                }
                output := {
                    result : SomeUnionWithUnit
                }
                errors: [
                    ValidationException
                ]
            }
            
            @length(min: 13)
            string StringRestricted
            
            union SomeUnionWithUnit {
                Option1: Unit
                Option2: StringRestricted
            }

            @http(uri: "/test", method: "POST")
            operation TestOperation {
                input := { payload: String }
                output := {
                    @httpPayload
                    events: TestEvent
                },
                errors: [ValidationException]
            }
            
            @streaming
            union TestEvent {
                KeepAlive: Unit,
                Response: TestResponseEvent,
            }
            
            structure TestResponseEvent { 
                data: String 
            }            
            """.asSmithyModel()

        serverIntegrationTest(model) { _, rustCrate ->
            rustCrate.testModule {
                rustTemplate(
                    """
                    use ::aws_smithy_eventstream::frame::MarshallMessage;
                    use aws_smithy_types::event_stream::HeaderValue;
                    
                    ##[macro_export]
                    macro_rules! assert_header {
                        (${'$'}msg:expr, ${'$'}name:expr, ${'$'}expected_value:expr) => {
                            let header = ${'$'}msg.headers().iter().find(|h| h.name().as_str() == ${'$'}name);
                            match header {
                                Some(h) => assert_eq!(*h.value(), HeaderValue::String(${'$'}expected_value.into())),
                                None => panic!("Header with name {} not found", ${'$'}name),
                            }
                        };
                    }
                    """,
                )

                unitTest("message_with_keep_alive") {
                    rustTemplate(
                        """
                        let event = crate::model::TestEvent::KeepAlive;
                        let result = crate::event_stream_serde::TestEventMarshaller::new().marshall(event);
                        assert!(result.is_ok(), "expected ok, got: {:?}", result);
                        let message = result.unwrap();
                        assert_header!(message, ":message-type", "event");
                        assert_header!(message, ":event-type", "KeepAlive");
                        assert_header!(message, ":content-type", "application/vnd.amazon.eventstream");
                        assert_eq!(&b"{}"[..], message.payload());
                        """,
                    )
                }

                unitTest("message_with_keep_response") {
                    rustTemplate(
                        """
                        let event = crate::model::TestEvent::Response(crate::model::TestResponseEvent::builder().data(Some("HelloWorld".to_string())).build());
                        let result = crate::event_stream_serde::TestEventMarshaller::new().marshall(event);
                        assert!(result.is_ok(), "expected ok, got: {:?}", result);
                        let message = result.unwrap();
                        assert_header!(message, ":message-type", "event");
                        assert_header!(message, ":event-type", "Response");
                        assert_header!(message, ":content-type", "application/vnd.amazon.eventstream");
                        assert_eq!(&b"{\"data\":\"HelloWorld\"}"[..], message.payload());
                        """,
                    )
                }

                unitTest("unconstrained_to_constrained") {
                    rustTemplate(
                        """
                        let unconstrained = crate::unconstrained::some_union_with_unit_unconstrained::SomeUnionWithUnitUnconstrained::Option1;
                        let constrained = crate::model::SomeUnionWithUnit::try_from(unconstrained);
                        assert!(constrained.is_ok(), "could not convert from unconstrained::SomeUnionWithUnconstrained, error: {:?}", constrained);
                        let unconstrained = crate::unconstrained::some_union_with_unit_unconstrained::SomeUnionWithUnitUnconstrained::Option2("Hello, World!".to_owned());
                        let constrained = crate::model::SomeUnionWithUnit::try_from(unconstrained);
                        assert!(constrained.is_ok(), "could not convert from unconstrained::SomeUnionWithUnconstrained, error: {:?}", constrained);
                        let constrained_value = constrained.as_ref().unwrap().as_option2().expect("Should have been converted to Option2");
                        assert_eq!(constrained_value.as_str(), "Hello, World!");
                        """,
                    )
                }
            }
        }
    }
}
