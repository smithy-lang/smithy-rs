RFC: Disable Payload Signing
=============================

> Status: RFC
>
> Applies to: client

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC proposes a way for users to disable payload signing for requests with a streaming binary payload.


The user experience if this RFC is implemented
----------------------------------------------


In the current version of the SDK, users are unable to control how a request is signed.
Most requests must include a hash of the payload as part of the signature. However, requests with a
[streaming](https://smithy.io/2.0/spec/streaming.html#smithy-api-streaming-trait) `blob` shape as the payload can
sometimes (e.g. S3 `PutObject`) choose to skip payload hashing and exclude it from the signature. When calculating the
signature the body hash is instead set to the literal constant `UNSIGNED-PAYLOAD`. This has the advantage of not paying
the cost of reading the payload into memory or hashing it at the cost of giving up some integrity checks.
This may be a desired tradeoff though when performance is the top concern or when other checksums are already being used
(e.g. [S3 checksums](https://aws.amazon.com/blogs/aws/new-additional-checksum-algorithms-for-amazon-s3/)).



Once this RFC is implemented, users will have the ability to selectively disable payload signing by customizing the
request.

```rust,ignore
let resp = s3.put_object()
    .bucket(bucket_name)
    .key(key_name)
    .body(body)
    .customize()
    .disable_payload_signing();
```

This new `disable_payload_signing()` method will only be added selectively to requests with a binary stream as the
payload.


How to actually implement this RFC
----------------------------------

Each service client operation can be customized to (e.g.) override specific configuration for just that operation
invocation. See [RFC-0017](./rfc0017_customizable_client_operations.html)
for more details.

In each generated service a [`CustomizableOperation<T, E, B>`](https://github.com/awslabs/aws-sdk-rust/blob/release-2024-04-11/sdk/s3/src/client/customize.rs#L3)
is generated. These are exposed as part of the [fluent builder](https://github.com/awslabs/aws-sdk-rust/blob/release-2024-04-11/sdk/s3/src/operation/put_object/builders.rs#L152) for each operation.


A new `impl` block specific to only binary streaming requests will be generated that
exposes this new `disable_payload_signing()` method.

```rust,ignore

impl CustomizableOperation<
    crate::operation::put_object::PutObjectOutput,
    crate::operation::put_object::PutObjectError,
    PutObjectFluentBuilder
> {

    /// Disable payload signing for a single operation invocation.
    fn disable_payload_signing(mut self) -> Self {
        // register a plugin that overrides the default signing parameters
        // and sets `signing_options.payload_override` to `SignableBody::UnsignedPayload`
        self.runtime_plugins
            .push(::aws_runtime::auth::DisablePayloadSigningPlugin::new());
    }
}

```



Changes checklist
-----------------

- [ ] Create a new runtime plugin that disables payload signing
- [ ] Update codegen to generate a specific `impl` block for requests with a `@streaming blob` payload that
      registers the new plugin
- [ ] Add new integration test(s)
- [ ] Update developer guide(s)
