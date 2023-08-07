RFC: RequestID in business logic handlers
=============

> Status: Implemented
>
> Applies to: server

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

Terminology
-----------

- **RequestID**: a service-wide request's unique identifier
- **UUID**: a universally unique identifier

RequestID is an element that uniquely identifies a client request. RequestID is used by services to map all logs, events and
specific data to a single operation. This RFC discusses whether and how smithy-rs can make that value available to customers.

Services use a RequestID to collect logs related to the same request and see its flow through the various operations,
help clients debug requests by sharing this value and, in some cases, use this value to perform their business logic. RequestID is unique across a service at least within a certain timeframe.

This value for the purposes above must be set by the service.

Having the client send the value brings the following challenges:
* The client could repeatedly send the same RequestID
* The client could send no RequestID
* The client could send a malformed or malicious RequestID (like in [1](https://en.wikipedia.org/wiki/Shellshock_(software_bug)) and
[2](https://cwiki.apache.org/confluence/display/WW/S2-045)).

To minimise the attack surface and provide a uniform experience to customers, servers should generate the value.
However, services should be free to read the ID sent by clients in HTTP headers: it is common for services to
read the request ID a client sends, record it and send it back upon success. A client may want to send the same value to multiple services.
Services should still decide to have their own unique request ID per actual call.

RequestIDs are not to be used by multiple services, but only within a single service.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

The proposal is to implement a `RequestId` type and make it available to middleware and business logic handlers, through [FromParts](../server/from_parts.md) and as a `Service`.
To aid customers already relying on clients' request IDs, there will be two types: `ClientRequestId` and `ServerRequestId`.

1. Implementing `FromParts` for `Extension<RequestId>` gives customers the ability to write their handlers:

```rust,ignore
pub async fn handler(
    input: input::Input,
    request_id: Extension<ServerRequestId>,
) -> ...
```
```rust,ignore
pub async fn handler(
    input: input::Input,
    request_id: Extension<ClientRequestId>,
) -> ...
```

`ServerRequestId` and `ClientRequestId` will be injected into the extensions by a layer.
This layer can also be used to open a span that will log the request ID: subsequent logs will be in the scope of that span.

2. ServerRequestId format:

Common formats for RequestIDs are:

* UUID: a random string, represented in hex, of 128 bits from IETF RFC 4122: `7c038a43-e499-4162-8e70-2d4d38595930`
* The hash of a sequence such as `date+thread+server`: `734678902ea938783a7200d7b2c0b487`
* A verbose description: `current_ms+hostname+increasing_id`

For privacy reasons, any format that provides service details should be avoided. A random string is preferred.
The proposed format is to use UUID, version 4.

A `Service` that inserts a RequestId in the extensions will be implemented as follows:
```rust,ignore
impl<R, S> Service<http::Request<R>> for ServerRequestIdProvider<S>
where
    S: Service<http::Request<R>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<R>) -> Self::Future {
        req.extensions_mut().insert(ServerRequestId::new());
        self.inner.call(req)
    }
}
```

For client request IDs, the process will be, in order:
* If a header is found matching one of the possible ones, use it
* Otherwise, None

`Option` is used to distinguish whether a client had provided an ID or not.
```rust,ignore
impl<R, S> Service<http::Request<R>> for ClientRequestIdProvider<S>
where
    S: Service<http::Request<R>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<R>) -> Self::Future {
        for possible_header in self.possible_headers {
            if let Some(id) = req.headers.get(possible_header) {
                req.extensions_mut().insert(Some(ClientRequestId::new(id)));
                return self.inner.call(req)
            }
        }
        req.extensions_mut().insert(None);
        self.inner.call(req)
    }
}
```

The string representation of a generated ID will be valid for this regex:
* For `ServerRequestId`: `/^[A-Za-z0-9_-]{,48}$/`
* For `ClientRequestId`: see [the spec](https://httpwg.org/specs/rfc9110.html#rfc.section.5.5)

Although the generated ID is opaque, this will give guarantees to customers as to what they can expect, if the server ID is ever updated to a different format.

Changes checklist
-----------------

- [x] Implement `ServerRequestId`: a `new()` function that generates a UUID, with `Display`, `Debug` and `ToStr` implementations
- [ ] Implement `ClientRequestId`: `new()` that wraps a string (the header value) and the header in which the value could be found, with `Display`, `Debug` and `ToStr` implementations
- [x] Implement `FromParts` for `Extension<ServerRequestId>`
- [ ] Implement `FromParts` for `Extension<ClientRequestId>`

Changes since the RFC has been approved
---------------------------------------

This RFC has been changed to only implement `ServerRequestId`.
