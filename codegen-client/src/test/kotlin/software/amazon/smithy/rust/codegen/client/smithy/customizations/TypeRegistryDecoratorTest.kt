/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.condition.EnabledIf
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

/**
 * Codegen unit tests for [TypeRegistryDecorator].
 *
 * The decorator emits `Client::registry() -> &'static TypeRegistry` and a
 * `crate::type_registry::REGISTRY` static populated with every user-modeled
 * structure shape in the service closure. These tests run real codegen
 * against a hand-rolled model and exercise the resulting registry from
 * generated Rust integration tests — `clientIntegrationTest` invokes
 * `cargo test` on the produced crate, so any codegen mistake (invalid
 * Rust, missing entry, wrong shape filter) surfaces as a test failure.
 *
 * The assertions split into two layers:
 * - The compile-success of `clientIntegrationTest` confirms the decorator
 *   produced a syntactically valid `TypeRegistry` static, that every
 *   referenced `Type::SCHEMA` is in fact emitted, and that the type-erased
 *   `DeserializeFn` matches the schema's runtime shape.
 * - The Rust test bodies pin down the *contents* of the registry — which
 *   shapes appear, which don't, and that the registered deserialize fns
 *   round-trip a `Document` back to the concrete typed shape.
 *
 * Sibling suite: [ErrorRegistryDecoratorTest] for the error-registry
 * counterpart.
 */
@EnabledIf("schemaSerdeEnabled")
class TypeRegistryDecoratorTest {
    companion object {
        /**
         * `Client::registry()` is only generated when the service's protocol is on
         * [SchemaSerdeAllowlist]. This suite's model uses awsJson1_0, so it runs only
         * when that protocol is enabled for schema-serde and is skipped otherwise —
         * keeping the coverage live without hard-disabling it.
         */
        @JvmStatic
        fun schemaSerdeEnabled(): Boolean = SchemaSerdeAllowlist.isProtocolEnabled(AwsJson1_0Trait.ID)
    }

    private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> {
        val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
        val smithySchema = RuntimeType.smithySchema(runtimeConfig)
        return arrayOf(
            "Document" to smithyTypes.resolve("Document"),
            "DocumentObject" to smithyTypes.resolve("document::DocumentObject"),
            "DiscriminatedDocument" to smithyTypes.resolve("DiscriminatedDocument"),
            "Number" to smithyTypes.resolve("Number"),
            "shape_id" to smithySchema.resolve("shape_id"),
        )
    }

    /**
     * Coverage model: a single awsJson1_0 service exercising every kind of
     * shape the decorator must filter on.
     *
     * - `Bird`, `Habitat` — plain user-modeled structures, both should be
     *   in the registry (`Habitat` is reachable only through `Bird.habitat`,
     *   verifying that the closure walk picks up nested-only shapes).
     * - `BirdEvent` — a union shape; per the SEP, unions are excluded from
     *   the primary registry even though they share structural traits with
     *   structures.
     * - `BirdNotFound` — a `@error`-trait shape; per the SEP the primary
     *   registry includes every structure shape (errors included), so it
     *   must appear here as well as in the dedicated error registry.
     * - The synthetic operation input/output shapes for `GetBird` are
     *   produced by `OperationNormalizer`; the decorator's
     *   `SyntheticInputTrait` / `SyntheticOutputTrait` filter must exclude
     *   them so third-party callers can't look them up via the registry
     *   under their codegen-internal `…synthetic#…Input` IDs.
     */
    private val model =
        """
        namespace com.example
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service AviaryService {
            version: "2024-01-01",
            operations: [GetBird],
        }

        @optionalAuth
        operation GetBird {
            input: GetBirdInput,
            output: GetBirdOutput,
            errors: [BirdNotFound],
        }

        structure GetBirdInput {
            name: String,
        }

        structure GetBirdOutput {
            bird: Bird,
        }

        structure Bird {
            name: String,
            colors: ColorList,
            habitat: Habitat,
            event: BirdEvent,
        }

        structure Habitat {
            biome: String,
            altitude: Integer,
        }

        union BirdEvent {
            sighted: Sighted,
            departed: Departed,
        }

        structure Sighted {
            at: Timestamp,
        }

        structure Departed {
            at: Timestamp,
        }

        list ColorList {
            member: String,
        }

        @error("client")
        structure BirdNotFound {
            message: String,
        }
        """.asSmithyModel()

    @Test
    fun `registry contains user-modeled structures including nested-only ones`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("registry_contains_modeled_structures") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn registry_contains_modeled_structures() {
                        let registry = $moduleName::Client::registry();

                        // Bird is a top-level operation output member — present.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example", "Bird"))
                                .is_some(),
                            "Bird must be registered",
                        );

                        // Habitat is reachable only through Bird.habitat, never
                        // mentioned directly by an operation. Verifies that the
                        // service-closure walk picks up transitively-referenced
                        // shapes, not just direct operation I/O.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example", "Habitat"))
                                .is_some(),
                            "Habitat (transitively referenced) must be registered",
                        );

                        // Sighted and Departed are union-variant target structures.
                        // They're regular structures (not the union itself) so they
                        // should be registered.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example", "Sighted"))
                                .is_some(),
                            "Sighted (union variant target) must be registered",
                        );
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example", "Departed"))
                                .is_some(),
                            "Departed (union variant target) must be registered",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `registry excludes union shapes`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("registry_excludes_unions") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn registry_excludes_unions() {
                        let registry = $moduleName::Client::registry();

                        // Per SEP § "What to include in the package level Type
                        // Registries": unions are deliberately excluded even though
                        // they share structural traits with structures.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example", "BirdEvent"))
                                .is_none(),
                            "BirdEvent (union) must NOT be in the primary registry",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `registry includes error shapes`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("registry_includes_errors") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn registry_includes_errors() {
                        let registry = $moduleName::Client::registry();

                        // Per the SEP, the primary registry includes every structure
                        // shape — `@error` structures included. The same shape also
                        // appears in `Client::error_registry()` (see
                        // ErrorRegistryDecoratorTest); a shape legitimately lives in
                        // both registries.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example", "BirdNotFound"))
                                .is_some(),
                            "@error structures must appear in the primary registry",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `registry excludes synthetic operation input and output shapes`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("registry_excludes_synthetics") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn registry_excludes_synthetics() {
                        let registry = $moduleName::Client::registry();

                        // OperationNormalizer renames operation input/output shapes,
                        // putting them under a `com.example.synthetic` namespace with
                        // names like `GetBirdInput` and `GetBirdOutput`. They're
                        // codegen artifacts; the decorator's SyntheticInputTrait /
                        // SyntheticOutputTrait filter excludes them so callers can't
                        // accidentally look them up under codegen-internal IDs.
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example.synthetic", "GetBirdInput"))
                                .is_none(),
                            "synthetic operation input must NOT be in the registry",
                        );
                        assert!(
                            registry
                                .schema_for(&#{shape_id}!("com.example.synthetic", "GetBirdOutput"))
                                .is_none(),
                            "synthetic operation output must NOT be in the registry",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `registry deserialize_document round-trips a typed shape`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("registry_deserialize_document_round_trip") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn registry_deserialize_document_round_trip() {
                        // Build a Document carrying the data of a `Habitat` shape.
                        // Map keys must match Smithy member names (the schema's
                        // `member_name`), not the snake_case Rust field names.
                        let mut members = #{DocumentObject}::new();
                        members.insert("biome".to_owned(), #{Document}::String("forest".to_owned()));
                        members.insert(
                            "altitude".to_owned(),
                            #{Document}::Number(#{Number}::PosInt(1200)),
                        );
                        let doc = #{DiscriminatedDocument}::new(#{Document}::Object(members))
                            .with_discriminator("com.example##Habitat");

                        let typed = $moduleName::Client::registry()
                            .deserialize_document(&doc)
                            .expect("Habitat is registered and the document is well-formed");

                        let habitat = typed
                            .downcast::<$moduleName::types::Habitat>()
                            .expect("DeserializeFn returns the concrete Habitat type");

                        assert_eq!(habitat.biome.as_deref(), Some("forest"));
                        assert_eq!(habitat.altitude, Some(1200));
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `registry returns error for unknown discriminator`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("registry_unknown_discriminator_errors") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn registry_unknown_discriminator_errors() {
                        // A Document with a discriminator that points to a shape NOT
                        // in this service's closure must produce an error. This is
                        // the negative path that complements the round-trip test.
                        let doc = #{DiscriminatedDocument}::new(
                            #{Document}::Object(#{DocumentObject}::new()),
                        )
                        .with_discriminator("com.example##NotAModeledShape");

                        let result = $moduleName::Client::registry().deserialize_document(&doc);
                        assert!(result.is_err(), "unregistered shape must error");
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `registry returns error for document without discriminator`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.integrationTest("registry_missing_discriminator_errors") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.Test.render(this)
                rustTemplate(
                    """
                    fn registry_missing_discriminator_errors() {
                        // Without a discriminator, the registry has no way to choose
                        // a deserialize fn. Surface as an error rather than silently
                        // pick a default — this keeps the runtime contract explicit.
                        let doc = #{DiscriminatedDocument}::new(
                            #{Document}::Object(#{DocumentObject}::new()),
                        );

                        let result = $moduleName::Client::registry().deserialize_document(&doc);
                        assert!(
                            result.is_err(),
                            "missing-discriminator path must error",
                        );
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }
}
