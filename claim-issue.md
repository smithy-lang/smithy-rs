# Multi-protocol REST route claiming and request content type

## Problem

REST XML protocol claiming in multi-protocol mode currently does not have enough
operation-specific information to decide whether a request should be claimed by
REST XML.

The Smithy REST XML protocol identification rule is operation-specific. A REST
XML server should claim a request when:

- the HTTP method and URI path match an operation route, and
- the request `Content-Type` matches the operation's expected request content
  type.

Today, the generated multi-protocol server can route by method, path, and query,
but the route matching layer does not know the operation's expected request
content type.

## Current behavior

`RequestSpec` represents generic HTTP binding information:

- HTTP method
- URI path pattern
- query requirements

`RestRouter` stores `RequestSpec` values and uses them to match a route. This is
shared by REST JSON and REST XML.

That means `RestRouter::match_route` can answer:

> Does this request match an operation's REST HTTP binding?

It cannot answer:

> Does this request match this operation's expected request `Content-Type` for
> this protocol?

The generated REST JSON and REST XML route specs are separate per protocol
router, but they are still the same generic `RequestSpec` shape. For operations
with the same HTTP binding, the REST JSON and REST XML route specs are
structurally identical.

## Why single-protocol validation is not enough

Single-protocol REST XML servers do validate request `Content-Type`, but that
happens after routing, inside the generated `FromRequest<RestXml, B>` path.

The single-protocol flow is:

1. Route the request by method, path, and query.
2. Dispatch to the selected operation.
3. Run the generated REST XML request deserializer.
4. Validate the operation's request `Content-Type` while deserializing.
5. Return a REST XML `UnsupportedMediaType` error if validation fails.

That works for a single-protocol server because every matched route is already
known to be REST XML.

In a multi-protocol server, the earlier question is different:

> Should REST XML claim this request at all, or should another protocol get a
> chance to claim it?

The current router/detector boundary cannot answer that because the detector can
only ask whether the route exists. It cannot ask whether the route exists and the
request content type is valid for that operation under REST XML.

## Desired behavior

For REST XML in multi-protocol mode:

- If method/path/query do not match, REST XML should not claim.
- If method/path/query match and `Content-Type` matches the operation's expected
  request content type, REST XML should claim and dispatch.
- If method/path/query match but `Content-Type` is wrong, REST XML should not
  immediately claim if another protocol can correctly claim the request.
- If no later protocol claims the request, the server should still be able to
  return the protocol-correct REST XML error, such as `415 UnsupportedMediaType`,
  instead of falling through to a generic `404`.

The behavior for an absent `Content-Type` when no other protocol matches still
needs a separate decision. It depends on whether the operation expects a request
document or payload, whether the request body is empty, and how much leniency we
want to preserve from the existing generated deserialization path.

## Possible design

Introduce a richer protocol claim result instead of representing detection as
`Option<DetectionResult<S>>`.

Conceptually:

```rust
enum ProtocolClaim<S, R> {
    NoClaim,
    RouteMatched(S),
    Claimed,
    Rejected(R),
    RejectedNonExclusive(R),
}
```

The variants describe what the protocol knows about the request. They should not
encode outer service control flow:

- `NoClaim` means this protocol has no claim on the request.
- `RouteMatched` means this protocol claims the request and has already resolved
  the operation route.
- `Claimed` means this protocol claims the request, but operation routing still
  needs to run.
- `Rejected` means this protocol rejects the request and no later protocol
  should be tried.
- `RejectedNonExclusive` means this protocol rejects the request according to
  its own rules, but the rejection is not exclusive; later protocols may still
  be tried.

The outer multi-protocol service decides what to do with each result:

```rust
match claim {
    ProtocolClaim::NoClaim => call_inner(),
    ProtocolClaim::RouteMatched(route) => dispatch(route),
    ProtocolClaim::Claimed => route_or_return_protocol_error(),
    ProtocolClaim::Rejected(reason) => return_error(reason),
    ProtocolClaim::RejectedNonExclusive(reason) => {
        maybe_store_fallback(reason);
        call_inner()
    }
}
```

For REST XML:

- a valid route and valid operation request content type should produce
  `RouteMatched`.
- no matching route signal should produce `NoClaim`.
- a route match with the wrong operation request content type should produce
  `RejectedNonExclusive`.
- a path match with the wrong method may also be `RejectedNonExclusive` in a
  multi-protocol REST chain, since another protocol could still claim the same
  HTTP request.

For protocols with exclusive protocol-identifying headers, such as AWS JSON and
RPCv2 CBOR, invalid method or unknown operation can remain exclusive. Those
protocols can use `Claimed` and let their router return protocol-specific route
errors, or use `Rejected` when detection itself can produce the final rejection.

The route metadata needs to include enough operation-specific information to
evaluate the request content type. Keep `RequestSpec` generic and wrap it in
REST-specific route metadata instead of adding protocol-specific content-type
state directly to `RequestSpec`.

Conceptually:

```rust
struct RestRouteSpec {
    request_spec: RequestSpec,
    request_content_type: RequestContentTypeClaim,
}

enum RequestContentTypeClaim {
    Expected(&'static str),
    AnyValidContentType { default: &'static str },
}
```

`RequestSpec` continues to mean method/path/query route shape.
`RestRouteSpec` means REST route shape plus protocol-claiming metadata.

For `Expected(expected)`, the request must contain a syntactically valid
`Content-Type` header whose media type essence matches the Smithy-derived
expected content type. Matching should follow normal HTTP media type comparison:
type/subtype comparison is case-insensitive and parameters do not affect the
essence match.

For `AnyValidContentType { default }`, the request must contain a syntactically
valid `Content-Type` header, but it does not need to match `default`. This
applies when the operation input binds `Content-Type` with `@httpHeader`, so the
operation allows a custom content type. The `default` remains useful metadata
for diagnostics and policy, but is not an equality requirement.

For REST XML, the default expected request content type is `application/xml`,
but codegen should derive the claim policy per operation from the Smithy model.
For example, an `@httpPayload` blob with `@mediaType("fahad/awsome")` should
produce `Expected("fahad/awsome")`, while an input member bound with
`@httpHeader("Content-Type")` should produce `AnyValidContentType`.

Codegen should generate `RestRouteSpec` per REST operation using the same
Smithy-derived request content-type logic used by generated request
deserialization:

- REST XML default: `Expected("application/xml")`.
- REST JSON default: `Expected("application/json")`.
- `@httpPayload` with `@mediaType`: `Expected("<derived media type>")`.
- request event stream: `Expected("application/vnd.amazon.eventstream")`.
- `@httpHeader("Content-Type")`: `AnyValidContentType { default:
  "<derived/default content type>" }`.

Detection and deserialization should derive from the same model facts so they do
not drift.

The detection-stage content-type check is header-only. It decides whether a REST
protocol should claim the request in the multi-protocol chain. The generated
`FromRequest` implementation still performs operation deserialization
validation after a protocol and route have been selected.

`RestRouter` should expose a richer REST-specific claiming method in addition to
the existing generic route lookup:

```rust
impl<S> RestRouter<S> {
    fn claim_route<B>(&self, request: &http::Request<B>) -> RestRouteClaim<S>;
}
```

`match_route` can remain the generic router API. `claim_route` is for
multi-protocol REST detection, where the caller needs route rank, method
mismatch, and request content-type claim results.

`claim_route` should return REST-level facts, not protocol-specific responses:

```rust
enum RestRouteClaim<S> {
    NoClaim,
    RouteMatched {
        route: S,
        route_rank: usize,
    },
    RejectedNonExclusive {
        route_rank: usize,
        kind: RejectionKind,
        status_hint: StatusCode,
        reason: RestClaimRejection,
    },
}

enum RestClaimRejection {
    MethodNotAllowed,
    MissingContentType,
    InvalidContentType,
    UnexpectedContentType {
        expected: &'static str,
        found: String,
    },
}
```

The REST XML and REST JSON detector implementations should convert
`RestRouteClaim` into `ProtocolClaim`, including the deferred protocol-correct
response for `RejectedNonExclusive`.

`RestClaimRejection` is an internal classification, not a new client-facing
error model. Protocol detectors should map it onto existing protocol router and
runtime errors:

- `MethodNotAllowed` maps to the existing REST router `MethodNotAllowed`
  response for the selected protocol.
- `MissingContentType`, `InvalidContentType`, and `UnexpectedContentType` map to
  the selected protocol's existing `UnsupportedMediaType` runtime error.
- `NotAcceptable`, if used, maps to the selected protocol's existing
  `NotAcceptable` runtime error.
- `UnknownOperation`, if used, maps to the existing REST router `NotFound`
  response for the selected protocol.

The deferred response closure should call the existing `IntoResponse<Protocol>`
implementations while the protocol detector still knows the concrete protocol
type.

The generic multi-protocol service should not need to know whether a protocol is
REST-based. Instead, the detector trait should be parameterized over the router
type, and each protocol detector should decide which router API it needs:

```rust
trait ProtocolDetector<B, S, Rtr> {
    type Rejection;

    fn claim(&self, req: &Request<B>, router: &Rtr)
        -> ProtocolClaim<S, Self::Rejection>;
}
```

The outer `ProtocolService` simply calls `self.protocol.claim(&req,
&self.router)`. REST XML and REST JSON detector implementations can call
`RestRouter::claim_route`; header-identified protocol implementations can keep
using their own header and router rules. This keeps REST-specific behavior out
of the generic protocol service.

## Fallback rejection

The existing multi-protocol chain ends in a terminal service that returns a
default `404` when every protocol delegates.

We can use that terminal point to preserve `RejectedNonExclusive` results. Store
them in a private request extension as an aggregate:

```rust
struct NonExclusiveProtocolRejections {
    items: Vec<NonExclusiveProtocolRejection>,
}

struct NonExclusiveProtocolRejection {
    protocol_id: ShapeId,
    route_rank: usize,
    status_hint: StatusCode,
    kind: RejectionKind,
    into_response: Box<dyn FnOnce() -> http::Response<BoxBody> + Send>,
}

enum RejectionKind {
    MethodNotAllowed,
    UnsupportedMediaType,
    NotAcceptable,
    UnknownOperation,
}
```

`kind`, `status_hint`, `route_rank`, and `protocol_id` are structured metadata
for ranking, logging, and policy decisions. `into_response` delays construction
of the protocol-correct response until the terminal service actually chooses to
return it.

The extension type should remain private so downstream code cannot name it and
therefore cannot read or overwrite it through `Request::extensions`.

The flow is:

1. A protocol rejects the request according to its own rules, but reports
   `RejectedNonExclusive` because another protocol may still claim it.
2. The outer service appends a fallback rejection to the private request
   extension and delegates.
3. Later protocols still get a chance to claim the request.
4. If no protocol claims the request, the terminal service reads the fallback
   rejection.
5. If a fallback rejection exists, it returns that protocol-specific response.
   Otherwise, it returns the default `404`.

This allows REST XML to avoid stealing requests that another protocol can handle
while still returning a useful REST XML error when REST XML was the only
plausible protocol.

The first pass should use the stored fallback rejection in the terminal service.
If at least one fallback rejection exists, the terminal service returns the best
one; otherwise it returns the default `404`.

When multiple protocols record fallback rejections, the terminal service should
choose the rejection with the highest `route_rank`. `route_rank` should reuse
the existing `RequestSpec::rank()` value, which is already used by `RestRouter`
to prefer more specific REST routes during normal route matching. Ties should
keep the first recorded rejection, preserving protocol detection order.

## Future work: non-exclusive route matches

Some requests may be acceptable to a REST protocol but still not be exclusive
enough to claim immediately. For example, a payloadless operation with no
`Content-Type` might be handled by REST XML in a single-protocol server, but in
a multi-protocol server the same method/path could also be handled by REST JSON.

A future extension could add a claim result such as:

```rust
RouteMatchedNonExclusive(S)
```

This would mean the protocol can handle the request, but later protocols should
still be tried first. If no later protocol claims the request, the outer service
could dispatch one of the stored non-exclusive route matches.

This is deferred because storing rejected responses is much simpler than storing
matched routes. A matched route has a generic service type `S`, and a single
vector cannot store different concrete `S` types. The likely implementation path
is to box route services before storing them, so the fallback list can hold a
uniform boxed route type. That route fallback machinery is not needed for the
first pass.

## Open questions

- Should absent `Content-Type` count as a rejected REST XML claim when the route
  matches?
- Should route claiming preserve the existing generated deserializer leniency for
  empty request bodies?
