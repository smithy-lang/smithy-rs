# RFC: Server protocol claims and multiprotocol dispatch

> Status: RFC
>
> Applies to: server (`codegen.schemaSerde = true`, HTTP 1.x)

Serve every protocol declared on a Smithy service through one schema-driven
Tower service and one protocol-independent operation handler table. Runtime
protocol implementations own identification, routing, serde, and error framing.

## Service construction

Generated SDKs emit canonical `ServiceSchema`/`OperationSchema` descriptors and
handler bindings. `MultiProtocolRoutingServiceBuilder` resolves protocol shape
IDs against `ProtocolRegistry::BUILTIN` and decorator-contributed registries.
Factories receive shared `ServiceConfig` and their own
`customizationConfig.protocols.<shape ID>` settings.

`ProtocolOrder::{Before, After}` defines a precedence DAG across all registered
protocols. Transitive closure includes unserved registrations; served protocols
must be pairwise ordered. Missing/duplicate registrations, unresolved references,
cycles, incomparable served protocols, and invalid bindings fail construction.
Declaration and registry iteration order have no dispatch semantics.

Built-in precedence:

```text
rpcv2Cbor → awsJson1_0 → awsJson1_1 → restJson1 → restXml
```

## Claim contract

Each request is passed to the registered protocols in priority order to
claim it. There are two kinds of protocol routers supported:

1. `MetadataProtocolRouter`: these protocols can decide whether the request
   belongs to them or not, using the header alone.

2. `BodyProtocolRouter`: require the body to be collected before they can
   make the claim.

Each claim can be:

| Claim                            | Dispatch semantics                                                    |
| -------------------------------- | --------------------------------------------------------------------- |
| `ClaimedWithRoute(target)`       | Commit ownership; dispatch directly.                                  |
| `Claimed`                        | Commit ownership; resolve the operation. Routing errors are terminal. |
| `DeferredRejection(error)`       | Retain the first rejection; continue probing.                         |
| `NoClaim`                        | Continue probing.                                                     |
| `NeedsBodyToClaim` (body router) | Collect, then invoke `claim_with_body`.                               |

The first claimant owns input deserialization, output/error serialization, and
routing-error framing. A later claimant supersedes a deferred rejection. With
no claimant, emit the first deferred rejection through its originating protocol;
otherwise return `404`, body `<UnknownOperationException/>\n`, no `Content-Type`.
Single-protocol services enforce the same claim contract.

## Identification

- **rpcv2Cbor:** `POST`, `Smithy-Protocol: rpc-v2-cbor`, known service/operation URI.
- **AWS JSON:** `POST /`, known `X-Amz-Target`, version-specific MIME; event-stream
  MIME is admitted only for event-stream inputs.
- **REST:** modeled method/URI/query bindings and present input MIME, including
  payload media types, configured aliases, and modeled `Content-Type` overrides.

Overlapping REST payload MIME and AWS JSON event-stream version ambiguity resolve
by protocol precedence.

## Body and handler dispatch

Collect routing bodies once under `ServiceRequestBodyConfig::for_routing`;
reuse immutable `Bytes` across probes and replay bytes/trailers to the handler.
Collection failures terminate dispatch. Body routers receive only operations
without streaming input or output. Metadata recognition of streaming input skips
`NeedsBodyToClaim`; prior head claims retain precedence.

Dispatch inserts `SelectedOperation` with the canonical operation schema,
selected protocol, and operation collection policy. Schema upgrade/serde and
handler middleware consume that selection. A single metadata protocol uses
synchronous claim dispatch without allocating a routing future.

## Changes checklist

- [ ] Registry resolution and precedence validation.
- [ ] Claim dispatch, deferred rejection, body replay, and streaming exclusion.
- [ ] Generated multiprotocol integration tests and protocol-specific wire assertions.
