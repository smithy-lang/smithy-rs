# Event-stream capability on `ServerProtocol`: Option-consts vs capability subtrait

> **DECIDED 2026-08-23: Option B (subtrait), with one amendment** — a third
> const `EVENT_STREAM_HTTP_CONTENT_TYPE` (the HTTP-level Content-Type of the
> streaming response is NOT uniform across protocols: awsJson keeps
> `application/x-amz-json-1.x` while restJson1/restXml/rpcv2Cbor use
> `application/vnd.amazon.eventstream`). See plan.md §2a for the final trait.

Companion to `specs/plan.md` §2a. The event-stream design is decided (generated
`Marshaller<P>` / `Unmarshaller<P>` per stream union, frame payloads through
`P::Codec`, exactly two protocol facts needed by the frame glue). The open
question is only **how a protocol that does not support event streams expresses
that** — and therefore where misuse is caught. Both options below assume the
decided architecture: associated functions, static dispatch, no
`payload_codec()` method (`Self::Codec` serves both body and frame payloads).
Smithy model validation (`eventStreamHttp` on the protocol trait definition) is
the first line of defense in both options; this choice decides the backstop.

---

## Option A — `Option`-consts on the base trait

### Trait (runtime crate, one trait)

```rust
pub trait ServerProtocol: ProtocolShape {
    type Codec: Codec;
    fn codec() -> &'static Self::Codec;

    fn deserialize_request<T: DeserializableShape>(/* schema, request */) -> Result<T, ...>;
    fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody>;
    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody>;

    /// `None` = this protocol does not support event streams.
    const EVENT_PAYLOAD_CONTENT_TYPE: Option<&'static str>;
    const FRAMES_INITIAL_MESSAGES: bool;
}
```

### Per-marker impls (runtime crate)

```rust
impl ServerProtocol for RestJson1 {
    // ...three verbs + codec...
    const EVENT_PAYLOAD_CONTENT_TYPE: Option<&'static str> = Some("application/json");
    const FRAMES_INITIAL_MESSAGES: bool = false;
}

impl ServerProtocol for AwsJson1_0 {
    // ...three verbs + codec...
    const EVENT_PAYLOAD_CONTENT_TYPE: Option<&'static str> = None; // dead value
    const FRAMES_INITIAL_MESSAGES: bool = false;                   // dead value
}
```

Every protocol must write both consts, meaningful or not.

### Generated per-service code (event-stream op)

Frame glue is bounded on plain `ServerProtocol` and must unwrap at every use:

```rust
pub struct CapturePokemonEventsMarshaller<P: ServerProtocol>(PhantomData<P>);

impl<P: ServerProtocol> MarshallMessage for CapturePokemonEventsMarshaller<P> {
    fn marshall(&self, input: Self::Input) -> Result<Message, Error> {
        let content_type = P::EVENT_PAYLOAD_CONTENT_TYPE
            .ok_or_else(|| Error::marshalling("protocol does not support event streams"))?;
        // headers: :message-type / :event-type / :content-type(content_type)
        // payload: P::codec().create_serializer() → write_struct(SCHEMA, &inner)
    }
}
```

Same `ok_or_else` in the unmarshaller and in the initial-message glue. This is
the client's shipped pattern (`protocol.payload_codec().ok_or_else(||
"protocol has no payload codec")` in the pokemon snapshots).

### Request trace — where misuse is caught (✗)

```
POST /capture-pokemon-event/{region}          (protocol P chosen at assembly)
  │
  ▼
router ── matches op ──► protocol stack (monomorphized over P) ──► Upgrade<P, Input, S>
  │                                                                   │
  │                              FromRequest<P>: P::deserialize_request::<Input>(INPUT_SCHEMA, ...)
  │                                │  prelude members (REST: URI/headers; RPC: initial-request
  │                                │  frame when P::FRAMES_INITIAL_MESSAGES)
  │                                │  attach Receiver(Unmarshaller::<P>::new(), body)
  │                                │        ✗ A: P::EVENT_PAYLOAD_CONTENT_TYPE == None
  │                                │          → runtime error on FIRST FRAME (stream error /
  │                                │            malformed-request rejection), per request
  │                                ▼
  │                             handler(Input) → Result<Output, Error>
  │                                │
  │              Output: prelude via schema + body = Marshaller::<P> frame stream
  │                                │        ✗ A: same None check fails marshalling each frame
  │              pre-first-event error: HTTP error w/ eventstream content-type (A2 golden)
  ▼
http::Response
```

The unsupported-protocol case compiles everywhere and surfaces as a per-request
runtime failure inside the stream machinery.

---

## Option B — capability subtrait

### Traits (runtime crate, two traits)

```rust
pub trait ServerProtocol: ProtocolShape {
    type Codec: Codec;
    fn codec() -> &'static Self::Codec;

    fn deserialize_request<T: DeserializableShape>(/* schema, request */) -> Result<T, ...>;
    fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody>;
    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody>;
}

/// Implemented only by protocols whose Smithy definition declares
/// `eventStreamHttp`.
pub trait EventStreamProtocol: ServerProtocol {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str; // no Option
    const FRAMES_INITIAL_MESSAGES: bool;
}
```

### Per-marker impls (runtime crate)

```rust
impl ServerProtocol for RestJson1 { /* three verbs + codec */ }
impl EventStreamProtocol for RestJson1 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const FRAMES_INITIAL_MESSAGES: bool = false;
}

impl ServerProtocol for AwsJson1_0 { /* three verbs + codec — and nothing else */ }
```

A non-supporting protocol writes nothing event-stream-related at all.

### Generated per-service code (event-stream op)

Frame glue and the op's specialized impls carry the tighter bound; ordinary
operations stay on `ServerProtocol`:

```rust
pub struct CapturePokemonEventsMarshaller<P: EventStreamProtocol>(PhantomData<P>);

impl<P: EventStreamProtocol> MarshallMessage for CapturePokemonEventsMarshaller<P> {
    fn marshall(&self, input: Self::Input) -> Result<Message, Error> {
        // :content-type is just P::EVENT_PAYLOAD_CONTENT_TYPE — no failure path
        // payload: P::codec().create_serializer() → write_struct(SCHEMA, &inner)
    }
}

impl<P: EventStreamProtocol> FromRequest<P, B> for CapturePokemonInput { ... }
impl<P: EventStreamProtocol> IntoResponse<P> for CapturePokemonOutput { ... }
// every non-stream op: impl<P: ServerProtocol> ...
```

Generated crates instantiate with concrete markers, so the bound never appears
in user-facing signatures (handlers, plugins, builder API unchanged).

### Request trace — where misuse is caught (✗)

```
✗ B: ASSEMBLY TIME — instantiating the CapturePokemon stack with a P that lacks
     `impl EventStreamProtocol` fails to COMPILE (trait bound not satisfied).
     No such service binary exists; the trace below only occurs for valid P.

POST /capture-pokemon-event/{region}
  │
  ▼
router ──► protocol stack (P: EventStreamProtocol proven at compile time) ──► Upgrade<P, Input, S>
  │                                │
  │             FromRequest<P>: prelude (REST: URI/headers; RPC: initial-request
  │             frame — P::FRAMES_INITIAL_MESSAGES, statically known)
  │             attach Receiver(Unmarshaller::<P>::new(), body)   — no failure path
  │                                ▼
  │                             handler(Input) → Result<Output, Error>
  │                                │
  │             Output: prelude via schema + Marshaller::<P> frame stream,
  │             :content-type = P::EVENT_PAYLOAD_CONTENT_TYPE (infallible)
  │             pre-first-event error: HTTP error w/ eventstream content-type (A2 golden)
  ▼
http::Response
```

---

## Exact issues

| Issue | A: Option-consts | B: subtrait |
|---|---|---|
| Failure timing | per-request runtime error inside stream machinery (stream error frame / rejection / 500) | compile error at assembly; no runtime path exists |
| Frame glue | `ok_or_else` unwrap at every const use (marshaller, unmarshaller, initial-message) | consts are plain values; no failure handling |
| Impl burden per protocol | every protocol writes both consts, dead values for non-supporters | non-supporters write nothing |
| Public API surface | one trait | two traits (`EventStreamProtocol` visible in docs and in event-stream op bounds) |
| Uniformity | all impls look alike | event-stream ops carry a different bound than plain ops (capability knowledge, model-derivable — does not violate plan principle 1) |
| Discoverability | reader must know `None` means unsupported | capability is legible from the impl list of each marker |
| Multiprotocol crate pairing supporting + non-supporting protocol with an event-stream op | compiles; the non-supporting nested stack fails per request | that stack does not compile — the invalid combination cannot be built (model validation should have rejected it upstream; this is the backstop) |
| Smithy validation interaction | backstop is runtime | backstop is the type system |

Orthogonal to this choice: `FRAMES_INITIAL_MESSAGES` may split into
`CONSUMES_INITIAL_REQUEST` / `EMITS_INITIAL_RESPONSE` once legacy server
initial-message behavior is verified per direction (plan §2a caveat). The split
lands identically on either option.

## Recommendation (decision is the user's)

**Option B.** It makes the invalid protocol×operation combination unrepresentable
instead of merely unlikely, deletes every `Option`-unwrap failure path from the
frame glue, and costs one additional, self-documenting public trait that never
reaches user-facing signatures. Option A's only advantages — a flat trait list
and impl uniformity — buy nothing at runtime that model validation hasn't
already promised.
