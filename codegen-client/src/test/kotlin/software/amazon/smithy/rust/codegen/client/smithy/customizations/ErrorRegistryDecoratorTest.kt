/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

/**
 * Codegen unit tests for [ErrorRegistryDecorator].
 *
 * The decorator produces two layers of registry, both of which need
 * coverage:
 * - A *service-wide* error registry at `crate::error_type_registry`
 *   exposed via `Client::error_registry()`. It contains every `@error`
 *   structure in the service closure and is the lookup target for
 *   third-party `Document`-based error reification.
 * - A *per-operation* error registry at
 *   `crate::operation::<op>::error_registry::REGISTRY`, populated only
 *   with the operation's declared errors and emitted only when the
 *   operation has at least one. Customers handling a `Document` known
 *   to belong to a specific operation use this for scoped dispatch.
 *
 * Sibling suite: [TypeRegistryDecoratorTest] for the primary registry.
 *
 * As with that suite, `clientIntegrationTest` invokes `cargo test` on
 * the generated crate, so a codegen mistake surfaces as either a Rust
 * compile failure (e.g. a missing static or wrong path) or a runtime
 * test failure.
 */
class ErrorRegistryDecoratorTest {
    /**
     * Both registry layers only exist when the service is generated with schema serde
     * exclusively, so every test here depends on that wiring. The fixture model uses the
     * dedicated schema-serde test namespace, which is allowlisted independently of the
     * production service and protocol rollout state, so this is a hard invariant rather
     * than a condition to skip on.
     *
     * A previous version of this suite gated on
     * `SchemaSerdeAllowlist.isProtocolEnabled(AwsJson1_0Trait.ID)`, which is false while
     * the protocol allowlist is empty during rollout. All five tests were silently
     * skipped and asserted nothing.
     */
    private fun requireSchemaSerde(codegenContext: ClientCodegenContext) {
        check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            "this suite's fixture must be generated with schema serde exclusively; " +
                "check that its namespace is still allowlisted in SchemaSerdeAllowlist"
        }
    }

    private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> {
        val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
        val smithySchema = RuntimeType.smithySchema(runtimeConfig)
        return arrayOf(
            "Document" to smithyTypes.resolve("Document"),
            "HashMap" to RuntimeType.HashMap,
            "DiscriminatedDocument" to smithyTypes.resolve("DiscriminatedDocument"),
            "Number" to smithyTypes.resolve("Number"),
            "shape_id" to smithySchema.resolve("shape_id"),
        )
    }

    /**
     * Coverage model: a single awsJson1_0 service with two operations
     * exercising every error-registry behavior the decorator must handle.
     *
     * - `BirdNotFound`, `NestEmpty` — `@error` structures included in the
     *   service-wide error registry.
     * - `Bird`, `Egg` — non-error structures that must NOT appear in the
     *   error registry (they belong to the *primary* registry instead).
     * - `GetBird` declares both errors, so its per-op registry must hold
     *   both.
     * - `LayEgg` declares only `NestEmpty`, so its per-op registry must
     *   hold only that error — pinning down per-op scoping rather than a
     *   blanket service-wide list.
     * - `Sing` declares no errors at all. The decorator must skip
     *   emitting a per-op `error_registry` module for it; this test
     *   suite asserts the positive path (the registries that *do* exist
     *   contain only what they should). Asserting absence cleanly would
     *   require a compile-must-fail harness, which is out of scope here.
     */
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service AviaryService {
            version: "2024-01-01",
            operations: [GetBird, LayEgg, Sing],
        }

        @optionalAuth
        operation GetBird {
            input: GetBirdInput,
            output: GetBirdOutput,
            errors: [BirdNotFound, NestEmpty],
        }

        @optionalAuth
        operation LayEgg {
            input: LayEggInput,
            output: LayEggOutput,
            errors: [NestEmpty],
        }

        @optionalAuth
        operation Sing {
            input: SingInput,
            output: SingOutput,
        }

        structure GetBirdInput { name: String }
        structure GetBirdOutput { bird: Bird }

        structure LayEggInput { species: String }
        structure LayEggOutput { egg: Egg }

        structure SingInput { volume: Integer }
        structure SingOutput { decibels: Integer }

        structure Bird {
            name: String,
        }

        structure Egg {
            color: String,
        }

        @error("client")
        structure BirdNotFound {
            message: String,
        }

        @error("server")
        structure NestEmpty {
            message: String,
        }
        """.asSmithyModel()

    @Test
    fun `service-wide error registry contains every modeled error`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            requireSchemaSerde(codegenContext)
            rustCrate.integrationTest("error_registry_contains_modeled_errors") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn error_registry_contains_modeled_errors() {
                        let registry = $moduleName::Client::error_registry();

                        // Both modeled errors in the service closure must appear,
                        // regardless of which operations declare them. The error
                        // registry is *service-wide*, not operation-scoped.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "BirdNotFound"))
                                .is_some(),
                            "BirdNotFound must be in the error registry",
                        );
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "NestEmpty"))
                                .is_some(),
                            "NestEmpty must be in the error registry",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `service-wide error registry excludes non-error structures`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            requireSchemaSerde(codegenContext)
            rustCrate.integrationTest("error_registry_excludes_non_errors") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn error_registry_excludes_non_errors() {
                        let registry = $moduleName::Client::error_registry();

                        // Non-error structures live in the *primary* registry,
                        // not the error registry. Asserting absence here is
                        // the inverse of the TypeRegistryDecoratorTest "registry
                        // contains user-modeled structures" assertion — together
                        // they pin down the partition between the two registries.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "Bird"))
                                .is_none(),
                            "Bird (data structure) must NOT be in the error registry",
                        );
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "Egg"))
                                .is_none(),
                            "Egg (data structure) must NOT be in the error registry",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `per-operation error registry holds the operation's declared errors`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            requireSchemaSerde(codegenContext)
            // The per-operation error_registry module is `pub(crate)` — it
            // exists for codegen-internal dispatch and is not part of the
            // generated SDK's public API. Reach it through an in-crate unit
            // test (rustCrate.testModule writes into `src/lib.rs` rather
            // than `tests/`) so the path resolves.
            rustCrate.testModule {
                unitTest("per_op_error_registry_contains_op_errors") {
                    rustTemplate(
                        """
                        // GetBird declares two errors. Its per-op registry
                        // must hold exactly those two — but for this test we
                        // only assert presence to avoid coupling to the
                        // registry's internal iteration / size API.
                        let get_bird_reg = &crate::operation::get_bird::error_registry::REGISTRY;
                        assert!(
                            get_bird_reg
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "BirdNotFound"))
                                .is_some(),
                            "GetBird::error_registry must contain BirdNotFound",
                        );
                        assert!(
                            get_bird_reg
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "NestEmpty"))
                                .is_some(),
                            "GetBird::error_registry must contain NestEmpty",
                        );

                        // LayEgg declares only NestEmpty. The per-op registry
                        // must NOT contain BirdNotFound — this is the scoping
                        // assertion that distinguishes per-op registries from
                        // the service-wide one.
                        let lay_egg_reg = &crate::operation::lay_egg::error_registry::REGISTRY;
                        assert!(
                            lay_egg_reg
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "NestEmpty"))
                                .is_some(),
                            "LayEgg::error_registry must contain NestEmpty",
                        );
                        assert!(
                            lay_egg_reg
                                .schema_for(&#{shape_id}!("smithy.rust.codegen.test.schemaheaders", "BirdNotFound"))
                                .is_none(),
                            "LayEgg::error_registry must NOT contain BirdNotFound — \
                            the scoping must be per-op, not service-wide",
                        );
                        """,
                        *codegenScope(codegenContext.runtimeConfig),
                    )
                }
            }
        }
    }

    @Test
    fun `error registry deserialize_document round-trips an error variant`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            requireSchemaSerde(codegenContext)
            rustCrate.integrationTest("error_registry_round_trip") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn error_registry_round_trip() {
                        // Build a Document carrying a BirdNotFound error. As
                        // with the primary registry round-trip, the map keys
                        // must match the Smithy member name (`message`).
                        let mut members = #{HashMap}::new();
                        members.insert(
                            "message".to_owned(),
                            #{Document}::String("no bird with that name".to_owned()),
                        );
                        let doc = #{DiscriminatedDocument}::new(#{Document}::Object(members))
                            .with_discriminator("smithy.rust.codegen.test.schemaheaders##BirdNotFound");

                        let typed = $moduleName::Client::error_registry()
                            .deserialize_document(&doc)
                            .expect("BirdNotFound is registered and the document is well-formed");

                        let err = typed
                            .downcast::<$moduleName::types::error::BirdNotFound>()
                            .expect("registered DeserializeFn returns the BirdNotFound type");

                        assert_eq!(err.message(), Some("no bird with that name"));
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `error registry returns error for unknown discriminator`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            requireSchemaSerde(codegenContext)
            rustCrate.integrationTest("error_registry_unknown_discriminator") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn error_registry_unknown_discriminator() {
                        // A discriminator that points to a non-error shape
                        // (or a shape outside the closure entirely) must
                        // produce an error rather than silently fall back.
                        let doc = #{DiscriminatedDocument}::new(
                            #{Document}::Object(#{HashMap}::new()),
                        )
                        .with_discriminator("smithy.rust.codegen.test.schemaheaders##NotAModeledError");

                        let result = $moduleName::Client::error_registry()
                            .deserialize_document(&doc);
                        assert!(
                            result.is_err(),
                            "unregistered shape ID must produce an error",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    /**
     * An awsQueryCompatible service identifies `QueueMissing` by its `@awsQueryError` code in the
     * `x-amzn-query-error` header. The error registries must match that code, as the operation's own dispatch does.
     */
    private val queryCompatibleModel =
        """
        ${'$'}version: "2"
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#awsJson1_0
        use aws.protocols#awsQueryCompatible
        use aws.protocols#awsQueryError

        @awsQueryCompatible
        @awsJson1_0
        service QueueService {
            version: "1"
            operations: [PurgeQueue, SendMessage]
        }

        operation PurgeQueue {
            input := {}
            output := {}
            errors: [QueueMissing]
        }

        /// Models no errors.
        operation SendMessage {
            input := {}
            output := {}
        }

        @awsQueryError(code: "AWS.SimpleQueueService.NonExistentQueue", httpResponseCode: 400)
        @error("client")
        structure QueueMissing {
            message: String
        }
        """.asSmithyModel()

    @Test
    fun `error registries match the protocol's error codes`() {
        clientIntegrationTest(queryCompatibleModel) { codegenContext, rustCrate ->
            requireSchemaSerde(codegenContext)
            val runtimeConfig = codegenContext.runtimeConfig
            rustCrate.testModule {
                rustTemplate(
                    """
                    use crate::operation::purge_queue::PurgeQueueError;
                    use crate::types::error::QueueMissing;
                    use crate::Client;
                    use #{ProvideErrorMetadata};

                    const CODE: &str = "AWS.SimpleQueueService.NonExistentQueue";

                    fn client(query_error: Option<&str>) -> Client {
                        let mut response = #{http_1x}::Response::builder().status(400);
                        if let Some(query_error) = query_error {
                            response = response.header("x-amzn-query-error", query_error);
                        }
                        let body = r##"{"__type":"smithy.rust.codegen.test.schemaheaders##QueueMissing","message":"gone"}"##;
                        let response = response.body(#{SdkBody}::from(body)).unwrap();
                        let (http_client, _request) = #{capture_request}(Some(response));
                        let config = crate::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client)
                            .behavior_version_latest()
                            .build();
                        Client::from_conf(config)
                    }

                    fn reified<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a QueueMissing> {
                        error.source()?.downcast_ref::<QueueMissing>()
                    }

                    ##[test]
                    fn entries_carry_the_query_error_code() {
                        for registry in [
                            &*crate::error_type_registry::REGISTRY,
                            &*crate::operation::purge_queue::error_registry::REGISTRY,
                        ] {
                            let entry = registry.entry_for_error_code(CODE).expect("the code matches QueueMissing");
                            assert_eq!(entry.schema().shape_id().shape_name(), "QueueMissing");
                            assert_eq!(entry.error_code(), Some(CODE));
                            // The shape name still matches, for a response without the header.
                            assert!(registry.entry_for_error_code("QueueMissing").is_some());
                        }
                    }
                    """,
                    "ProvideErrorMetadata" to RuntimeType.provideErrorMetadataTrait(runtimeConfig),
                    "capture_request" to RuntimeType.captureRequest(runtimeConfig),
                    "http_1x" to CargoDependency.Http1x.toType(),
                    "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
                )

                tokioTest("the_modeling_operation_dispatches_on_the_query_error_code") {
                    rustTemplate(
                        """
                        let error = client(Some(&format!("{CODE};Sender"))).purge_queue().send().await.unwrap_err();
                        assert!(matches!(error.into_service_error(), PurgeQueueError::QueueMissing(_)));
                        """,
                    )
                }

                tokioTest("the_service_wide_registry_reads_the_query_error_code") {
                    rustTemplate(
                        """
                        let error = client(Some(&format!("{CODE};Sender"))).send_message().send().await.unwrap_err();
                        let error = error.into_service_error();
                        assert_eq!(error.code(), Some(CODE));
                        assert_eq!(reified(&error).expect("the code matches QueueMissing").message(), Some("gone"));
                        """,
                    )
                }

                tokioTest("the_service_wide_registry_reads_the_shape_name_without_the_header") {
                    rustTemplate(
                        """
                        let error = client(None).send_message().send().await.unwrap_err().into_service_error();
                        assert_eq!(error.code(), Some("QueueMissing"));
                        assert_eq!(reified(&error).expect("the name matches QueueMissing").message(), Some("gone"));
                        """,
                    )
                }
            }
        }
    }
}
