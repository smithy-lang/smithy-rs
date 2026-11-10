/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * The type and error registries of a client whose structures have `DocumentV2` companions, through the `Foo` model:
 * what they hold, reading V1 and V2 discriminated documents, and the error path's registry fallback.
 */
class DocumentV2RegistryTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#awsJson1_0

        @awsJson1_0
        service FooService {
            version: "1"
            operations: [Foo, Bar, Ping]
        }

        operation Foo {
            input := {
                payload: Document
                data: FooData
            }
            output := {
                payload: Document
                data: FooData
            }
            errors: [FooError]
        }

        /// Models `OtherError`, so the service-wide error registry holds it, and `Foo`'s doesn't.
        operation Bar {
            input := {}
            output := {}
            errors: [OtherError]
        }

        /// Models no errors.
        operation Ping {
            input := {}
            output := {}
        }

        structure FooData {
            @required
            body: Document
        }

        @error("client")
        structure FooError {
            message: String
            details: Document
        }

        @error("client")
        structure OtherError {
            message: String
            details: Document
        }
        """.asSmithyModel()

    @Test
    fun `registries read V1 and V2 documents and back the error fallback`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "this test must generate schema serde; check that its namespace is still in SchemaSerdeAllowlist"
            }
            val runtimeConfig = codegenContext.runtimeConfig
            val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
            rustCrate.testModule {
                rustTemplate(
                    """
                    use crate::operation::foo::FooError as FooOperationError;
                    use crate::types::error::{FooError, OtherError};
                    use crate::types::FooData;
                    use crate::Client;
                    use #{BigInteger};
                    use #{DiscriminatedDocument};
                    use #{Document};
                    use #{DocumentObject};
                    use #{DocumentV2};
                    use #{ErrorMetadata};
                    use #{ProvideErrorMetadata};
                    use #{TypeRegistry};
                    use std::str::FromStr;

                    const BIG: &str = "123456789012345678901234567890";

                    fn shape(name: &str) -> String {
                        ["smithy.rust.codegen.test.schemaheaders", name].join("##")
                    }

                    // `Document` holds this value as the nearest `f64`.
                    fn big() -> DocumentV2 {
                        DocumentV2::BigInteger(BigInteger::from_str(BIG).unwrap())
                    }

                    // Converts to the V1 string "AQID".
                    fn blob() -> DocumentV2 {
                        DocumentV2::Blob(vec![1, 2, 3])
                    }

                    fn sorted_ids(registry: &TypeRegistry) -> Vec<String> {
                        let mut ids: Vec<String> = registry.iter().map(|(id, _)| id.to_string()).collect();
                        ids.sort();
                        ids
                    }

                    fn v1_document(name: &str, members: Vec<(&str, Document)>) -> DiscriminatedDocument {
                        let members = members.into_iter().map(|(key, value)| (key.to_string(), value)).collect();
                        DiscriminatedDocument::new(Document::Object(members)).with_discriminator(shape(name))
                    }

                    fn v2_document(name: &str, members: Vec<(&str, DocumentV2)>) -> DiscriminatedDocument<DocumentV2> {
                        let members: DocumentObject = members.into_iter().map(|(key, value)| (key.to_string(), value)).collect();
                        DiscriminatedDocument::new_v2(members.into())
                            .try_with_discriminator(shape(name))
                            .unwrap()
                    }

                    ##[test]
                    fn the_registry_holds_every_structure_but_operation_inputs_and_outputs() {
                        assert_eq!(
                            sorted_ids(Client::registry()),
                            [shape("FooData"), shape("FooError"), shape("OtherError")]
                        );
                    }

                    ##[test]
                    fn the_error_registry_holds_only_errors() {
                        assert_eq!(sorted_ids(Client::error_registry()), [shape("FooError"), shape("OtherError")]);
                    }

                    ##[test]
                    fn v1_and_v2_documents_read_into_foo_data() {
                        let from_v1 = Client::registry()
                            .deserialize_document(&v1_document("FooData", vec![("body", Document::from("AQID"))]))
                            .unwrap()
                            .downcast::<FooData>()
                            .unwrap();
                        assert_eq!(from_v1.body, Document::from("AQID"));
                        // The `Document` reader upgrades the value for the companion.
                        assert_eq!(from_v1.body_v2(), Some(&DocumentV2::from("AQID")));

                        let from_v2 = Client::registry()
                            .deserialize_document(&v2_document("FooData", vec![("body", blob())]))
                            .unwrap()
                            .downcast::<FooData>()
                            .unwrap();
                        assert_eq!(from_v2.body_v2(), Some(&blob()));
                        assert_eq!(from_v2.body, Document::from("AQID"));
                        assert_eq!(from_v1, from_v2);
                    }

                    ##[test]
                    fn v1_and_v2_documents_read_into_foo_error() {
                        for registry in [Client::error_registry(), Client::registry()] {
                            let from_v1 = registry
                                .deserialize_document(&v1_document(
                                    "FooError",
                                    vec![("message", Document::from("oops")), ("details", Document::from("AQID"))],
                                ))
                                .unwrap()
                                .downcast::<FooError>()
                                .unwrap();
                            assert_eq!(from_v1.message(), Some("oops"));
                            assert_eq!(from_v1.details, Some(Document::from("AQID")));

                            let from_v2 = registry
                                .deserialize_document(&v2_document(
                                    "FooError",
                                    vec![("message", DocumentV2::from("oops")), ("details", blob())],
                                ))
                                .unwrap()
                                .downcast::<FooError>()
                                .unwrap();
                            assert_eq!(from_v2.message(), Some("oops"));
                            assert_eq!(from_v2.details_v2(), Some(&blob()));
                            assert_eq!(from_v2.details, Some(Document::from("AQID")));
                        }

                        let not_an_error = v2_document("FooData", vec![("body", blob())]);
                        assert!(Client::error_registry().deserialize_document(&not_an_error).is_err());
                    }

                    fn client(body: &str) -> Client {
                        let response = #{http_1x}::Response::builder()
                            .status(400)
                            .body(#{SdkBody}::from(body.to_string()))
                            .unwrap();
                        let (http_client, _request) = #{capture_request}(Some(response));
                        let config = crate::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client)
                            .behavior_version_latest()
                            .build();
                        Client::from_conf(config)
                    }

                    fn other_error_body() -> String {
                        format!(r##"{{"__type":"{}","message":"oops","details":{BIG}}}"##, shape("OtherError"))
                    }

                    // The source of an unhandled error, if the registry read it as `OtherError`.
                    fn reified<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a OtherError> {
                        error.source()?.downcast_ref::<OtherError>()
                    }
                    """,
                    "BigInteger" to RuntimeType.bigInteger(runtimeConfig),
                    "DiscriminatedDocument" to smithyTypes.resolve("DiscriminatedDocument"),
                    "Document" to RuntimeType.document(runtimeConfig),
                    "DocumentObject" to smithyTypes.resolve("document::DocumentObject"),
                    "DocumentV2" to RuntimeType.documentV2(runtimeConfig),
                    "ErrorMetadata" to RuntimeType.errorMetadata(runtimeConfig),
                    "ProvideErrorMetadata" to RuntimeType.provideErrorMetadataTrait(runtimeConfig),
                    "TypeRegistry" to RuntimeType.smithySchema(runtimeConfig).resolve("registry::TypeRegistry"),
                    "capture_request" to RuntimeType.captureRequest(runtimeConfig),
                    "http_1x" to CargoDependency.Http1x.toType(),
                    "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
                )

                tokioTest("foo_reads_its_own_error_into_the_companion") {
                    rustTemplate(
                        """
                        let body = format!(r##"{{"__type":"FooError","details":{BIG}}}"##);
                        let error = client(&body).foo().send().await.unwrap_err().into_service_error();
                        let FooOperationError::FooError(error) = error else {
                            panic!("expected FooError, got {error:?}");
                        };
                        assert_eq!(error.details_v2(), Some(&big()));
                        """,
                    )
                }

                tokioTest("foo_reads_an_error_only_another_operation_models") {
                    rustTemplate(
                        """
                        let error = client(&other_error_body()).foo().send().await.unwrap_err().into_service_error();
                        assert!(!error.is_foo_error());
                        assert_eq!(error.code(), Some("OtherError"));
                        let source = reified(&error).expect("the service-wide error registry reads OtherError");
                        assert_eq!(source.message(), Some("oops"));
                        assert_eq!(source.details_v2(), Some(&big()));
                        """,
                    )
                }

                tokioTest("an_operation_without_modeled_errors_uses_the_service_wide_registry") {
                    rustTemplate(
                        """
                        let error = client(&other_error_body()).ping().send().await.unwrap_err().into_service_error();
                        assert_eq!(error.code(), Some("OtherError"));
                        let source = reified(&error).expect("the service-wide error registry reads OtherError");
                        assert_eq!(source.details_v2(), Some(&big()));
                        """,
                    )
                }

                tokioTest("unreadable_and_unknown_errors_stay_generic") {
                    rustTemplate(
                        """
                        // `message` must be a string, and `Nope` isn't an error in the service.
                        for (body, code) in [
                            (r##"{"__type":"OtherError","message":5}"##, "OtherError"),
                            (r##"{"__type":"Nope","message":"oops"}"##, "Nope"),
                        ] {
                            let foo = client(body).foo().send().await.unwrap_err().into_service_error();
                            let ping = client(body).ping().send().await.unwrap_err().into_service_error();
                            let errors: [&(dyn std::error::Error + 'static); 2] = [&foo, &ping];
                            assert_eq!(foo.code(), Some(code));
                            assert_eq!(ping.code(), Some(code));
                            for error in errors {
                                // A generic error's source is its metadata.
                                assert!(error.source().unwrap().downcast_ref::<ErrorMetadata>().is_some(), "{body}");
                            }
                        }
                        """,
                    )
                }
            }
        }
    }
}
