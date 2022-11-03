<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: RequestID in business logic handlers
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

- **RequestID**: a service-wide unique identifier
- **UUID**: a universally unique identifier

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
RequestID is an element that uniquely identifies a client request. RequestID is used by services to map all logs, events and
specific data to a single operation. This RFC discusses whether and how smithy-rs can make that value available to customers.

Services use a RequestID to collect logs related to the same request and see its flow through the various operations,
help clients debug requests by sharing this value and, in some cases, use this value to perform their business logic. RequestID is unique across a service.

This value for the purposes above must be set by the service. Clients could send any value not conforming to the valid, chosen format,
avoid sending one altogether or exploit how some service is using that value
(like in [1](https://en.wikipedia.org/wiki/Shellshock_(software_bug)) and
[2](https://cwiki.apache.org/confluence/display/WW/S2-045)).
Having the client send the value bring the following challenges:
* The client could repeatedly send the same RequestID
* The client could send no RequestID
* The client could send a malformed or malicious RequestID

To minimise the attack surface and provide a uniform experience to customers, servers must generate the value.
Services are free to read the ID sent by clients in HTTP headers.

RequestIDs are not to be used by multiple services, but only within a service.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

The proposal is to implement a `RequestId` type and make it available to middleware and business logic handlers, through [FromParts](../server/from-parts.md) and as a `Service`.

1. Implementing `FromParts` for `Extension<RequestId>` gives customers the ability to write their handlers:

```rust
pub async fn handler(
    input: input::Input,
    request_id: Extension<RequestId>,
) -> ...
```

`RequestId` will be injected into the extensions by a layer.
This layer can also be used to open a span that will log the request ID: subsequent logs will be in the scope of that span.

2. RequestId format:

Common formats for RequestIDs are:

* UUID: a random string, represented in hex, of 128 bits from IETF RFC 4122: `7c038a43-e499-4162-8e70-2d4d38595930`
* The hash of a sequence such as `date+thread+server`: `734678902ea938783a7200d7b2c0b487`
* A verbose description: `current_ms+hostname+increasing_id`

For privacy reasons, any format that provides service details should be avoided. A random string is preferred.
The proposed format is to use UUID, version 4.

AWS Lambda sends a RequestID in the request context; there is no need to modify it.
There should be a `Service` for Lambda users that extracts the value and injects it as for non-Lambda other requests.

Changes checklist
-----------------

- [ ] Implement `RequestId`: a `new()` function that generates a UUID, with `Display`, `Debug` and `ToStr` implementations
- [x] Implement `FromParts` for `Extension<RequestId>`
- [ ] Implement a `Service` that injects `RequestId` in the extensions and one that is compatible with Lambda
