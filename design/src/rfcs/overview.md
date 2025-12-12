# RFCs

**What is an RFC?:** An RFC is a document that proposes a change to `smithy-rs` or the AWS Rust SDK. Request for Comments means a request for discussion and oversight about the future of the project from maintainers, contributors and users.

**When should I write an RFC?:** The AWS Rust SDK team proactively decides to write RFCs for major features or complex changes that we feel require extra scrutiny. However, the process can be used to request feedback on any change. Even changes that seem obvious and simple at first glance can be improved once a group of interested and experienced people have a chance to weigh in.

**Who can submit an RFC?:** An RFC can be submitted by anyone. In most cases, RFCs are authored by SDK maintainers, but everyone is welcome to submit RFCs.

**Where do I start?:** If you're ready to write and submit an RFC, please start a GitHub discussion with a summary of what you're trying to accomplish first. That way, the AWS Rust SDK team can ensure they have the bandwidth to review and shepherd the RFC through the whole process before you've expended effort in writing it. Once you've gotten the go-ahead, start with the [RFC template](./rfc_template.md).

## Previously Submitted RFCs

- [RFC-0001: AWS Configuration](./rfc0001_shared_config.md)
- [RFC-0002: Supporting multiple HTTP versions for SDKs that use Event Stream](./rfc0002_http_versions.md)
- [RFC-0003: API for Presigned URLs](./rfc0003_presigning_api.md)
- [RFC-0004: Retry Behavior](./rfc0004_retry_behavior.md)
- [RFC-0005: Service Generation](./rfc0005_service_generation.md)
- [RFC-0006: Service-specific middleware](./rfc0006_service_specific_middleware.md)
- [RFC-0007: Split Release Process](./rfc0007_split_release_process.md)
- [RFC-0008: Paginators](./rfc0008_paginators.md)
- [RFC-0009: Example Consolidation](./rfc0009_example_consolidation.md)
- [RFC-0010: Waiters](./rfc0010_waiters.md)
- [RFC-0011: Publishing Alpha to Crates.io](./rfc0011_crates_io_alpha_publishing.md)
- [RFC-0012: Independent Crate Versioning](./rfc0012_independent_crate_versioning.md)
- [RFC-0013: Body Callback APIs](./rfc0013_body_callback_apis.md)
- [RFC-0014: Fine-grained timeout configuration](./rfc0014_timeout_config.md)
- [RFC-0015: How Cargo "features" should be used in the SDK and runtime crates](./rfc0015_using_features_responsibly.md)
- [RFC-0016: Supporting Flexible Checksums](./rfc0016_flexible_checksum_support.md)
- [RFC-0017: Customizable Client Operations](./rfc0017_customizable_client_operations.md)
- [RFC-0018: Logging in the Presence of Sensitive Data](./rfc0018_logging_sensitive.md)
- [RFC-0019: Event Streams Errors](./rfc0019_event_streams_errors.md)
- [RFC-0020: Service Builder Improvements](./rfc0020_service_builder.md)
- [RFC-0021: Dependency Versions](./rfc0021_dependency_versions.md)
- [RFC-0022: Error Context and Compatibility](./rfc0022_error_context_and_compatibility.md)
- [RFC-0023: Evolving the new service builder API](./rfc0023_refine_builder.md)
- [RFC-0024: RequestID](./rfc0024_request_id.md)
- [RFC-0025: Constraint traits](./rfc0025_constraint_traits.md)
- [RFC-0026: Client Crate Organization](./rfc0026_client_crate_organization.md)
- [RFC-0027: Endpoints 2.0](./rfc0027_endpoints_20.md)
- [RFC-0028: SDK Credential Cache Type Safety](./rfc0028_sdk_credential_cache_type_safety.md)
- [RFC-0029: Finding New Home for Credential Types](./rfc0029_new_home_for_cred_types.md)
- [RFC-0030: Serialization And Deserialization](./rfc0030_serialization_and_deserialization.md)
- [RFC-0031: Providing Fallback Credentials on Timeout](./rfc0031_providing_fallback_credentials_on_timeout.md)
- [RFC-0032: Better Constraint Violations](./rfc0032_better_constraint_violations.md)
- [RFC-0033: Improving access to request IDs in SDK clients](./rfc0033_improve_sdk_request_id_access.md)
- [RFC-0034: The Orchestrator Architecture](./rfc0034_smithy_orchestrator.md)
- [RFC-0035: Sensible Defaults for Collection Values](./rfc0035_collection_defaults.md)
- [RFC-0036: Enabling HTTP crate upgrades in the future](./rfc0036_http_dep_elimination.md)
- [RFC-0037: The HTTP wrapper type](./rfc0037_http_wrapper.md)
- [RFC-0038: Retry Classifier Customization](./rfc0038_retry_classifier_customization.md)
- [RFC-0039: Forward Compatible Errors](./rfc0039_forward_compatible_errors.md)
- [RFC-0040: Behavior Versions](./rfc0040_behavior_versions.md)
- [RFC-0041: Improve client error ergonomics](./rfc0041_improve_client_error_ergonomics.md)
- [RFC-0042: File-per-change changelog](./rfc0042_file_per_change_changelog.md)
- [RFC-0043: Identity Cache Partitions](./rfc0043_identity_cache_partitions.md)
- [RFC-0045: Configurable Serde](./rfc0045_configurable_serde.md)
- [RFC-0046: Wire cached responses](./rfc0046_wire_cached_responses.md)
