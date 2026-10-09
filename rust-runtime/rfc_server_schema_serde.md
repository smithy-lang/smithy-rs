RFC: Schema-driven server serde
===============================

> Status: RFC
>
> Applies to: server (`codegen.schemaSerde = true`, HTTP 1.x)

Move protocol serde into runtime libraries. Generated SDKs supply static schema
descriptors and typed walkers; runtime protocols interpret HTTP bindings and
wire encodings. Object-safe serde interfaces permit one walker per shape across
protocols and codecs.

Schema and runtime contract
---------------------------

Codegen emits `Type::SCHEMA`, member indices and trait metadata, plus
`OperationSchema`/`ServiceSchema` descriptors. Schemas retain HTTP bindings,
wire names, timestamp formats, XML metadata, payload traits, and error identity.

`aws-smithy-schema` defines `ShapeSerializer`, `ShapeDeserializer`, aggregate
serialization traits, and codec interfaces. `aws-smithy-http-server` implements
HTTP binding adapters, protocol policy, and upgrade services. JSON/XML/CBOR
codecs implement format semantics; generated walkers implement Rust field and
variant access.

Upgrade services
----------------

With `schemaSerde`, generated services use `DynUpgrade` for ordinary operations
and `DynStreamingUpgrade` for operations with streaming input or output.
Both select the protocol per request rather than through a protocol type parameter.
Services without `schemaSerde` retain the legacy `Upgrade` path.

Routing supplies `SelectedOperation`: the canonical operation schema, selected
protocol, and body collection policy. Upgrade checks operation identity and uses
the selected protocol for input deserialization, output/error serialization, and
protocol rejection framing:

```text
HTTP request → binding readers/codec → generated consumer → input builder
typed output → generated member writer → binding splitter/codec → HTTP response
```

Request-part extractors use `FromParts<DynProtocol>`. Typed handler signatures
remain the same. Protocol selection and routing-body replay are defined in
[Server protocol claims and multiprotocol dispatch](rfc_server_protocol_claims.md).

Deserialization
---------------

Upgrade extracts request context, converts the HTTP request, validates protocol
headers, and collects the body under operation limits when `BodyDirective`
requires it. `deserialize_request(input_schema, request)` returns a
`dyn ShapeDeserializer`; `Op::Input::deserialize` drives the generated walker.

The walker creates a builder and calls `read_struct(schema, consumer)`. Runtime
readers enumerate schema members and invoke the consumer with a member-specific
deserializer. The consumer switches on `member_index()`, invokes typed `read_*`
methods, and assigns builder fields directly. Nested aggregates recurse through
their walkers; collections use element/entry consumers.

REST adapters demultiplex labels, queries, headers, prefix maps, and payloads.
Document members use the codec; blob/string payloads use raw-byte readers.
RPC adapters deserialize the document through their codec.

Constrained inputs accumulate unconstrained builders/collections/unions, then
validate through `build()`. Constraint violations retain modeled
`ValidationException` field paths. Parse, media-type, and accept failures remain
distinct `DeserializeError` variants rendered by the selected protocol.

Serialization
-------------

On success, upgrade calls `serialize_response(output_schema, &output)`.
Generated `SerializableStruct::serialize_members` emits schema/value pairs through
typed `write_*` methods in canonical model order; union walkers emit the active
variant. Runtime serializers own wire formatting.

REST `ResponseBindingSplitter` diverts header, prefix-header, response-code, and
payload bindings; remaining members flow to the codec. RPC protocols encode
body members through the codec. REST factories cache compiled response plans for
canonical output/error schemas; dynamic schemas compile on demand.

Protocol policy assembles status, MIME, and buffered body length. Modeled errors
use `HttpModeledError` plus protocol-specific discriminator framing.
Serialization failures produce the protocol's serialization-error response.

Configuration migration
-----------------------

`smithy-build-template.json` becomes a source of generated defaults for runtime
behavior. Codegen already emits `customizationConfig.protocols` into
`ProtocolOptions`: global body limits become typed `ServiceConfig`; per-protocol
sections become shape-ID-keyed `Document` values parsed by protocol factories.
`rpcV2CborAddCapitalizedRoute` folds into runtime `capitalizeRoutes`.

Expose runtime overrides on generated Rust builders through `ProtocolOptions`.
Protocol-setting precedence is Rust overrides → emitted defaults → protocol
defaults. Factories validate configuration at service construction.
Body limits/timeouts, JSON `validateSkippedValues`, XML validation/MIME policy,
and route aliases execute in Rust without SDK regeneration. The generated
`request_body_config` setter already supports body-policy replacement;
general protocol overrides remain to be exposed.

Move `alwaysSendEventStreamInitialResponse` from generated glue into runtime
service policy. Keep generation choices—`schemaSerde`, HTTP version, constrained
type exposure, and walker-affecting union settings—in codegen. Configuration is
fixed per constructed service instance.

Streaming
---------

`DynStreamingUpgrade` collects non-streaming inputs under the operation policy,
including operations with streaming output only. For streaming inputs, it passes
live bodies as `SdkBody` to generated
`StreamingOperationShape` glue. Ordinary bindings populate the builder; glue
attaches streaming blobs or event receivers before validation. Output glue
supplies a live body to `serialize_streaming_response`. Event marshallers and
unmarshallers apply the same walkers per frame using `EventStreamFraming`;
streaming inputs and outputs bypass whole-body collection.

Changes checklist
-----------------

- [ ] Schema descriptors and protocol-independent typed walkers.
- [ ] Runtime binding adapters, codecs, upgrade, and rejection framing.
- [ ] Generated Rust configuration overrides and runtime initial-response policy.
- [ ] Legacy parity tests for bindings, validation, payloads, errors, and streams.
