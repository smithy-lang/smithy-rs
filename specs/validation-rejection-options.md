# Validation-rejection seam: the design space behind plan §2d

> **DECIDED 2026-08-23: `Box<dyn HttpModeledError + Send>`** (user sign-off,
> explicitly over the legacy pre-serialized `String`/`Vec<u8>` bytes). Kept as
> the decision record. NOTE the bound gap below: `HttpModeledError` today has
> no `Debug`/`Display`/`Send` supertraits — they must be added deliberately at
> Checkpoint 3 (Display via the shapes' existing `std::error::Error`), not
> discovered by compile error.

Companion to `specs/plan.md` §2d. The problem: a constraint violation is
detected inside `FromRequest` — before any handler exists — and must become a
serialized ValidationException on the wire. Something has to carry "which
modeled error, with which field values" from the violation site to the
response. The carrier's type decides where serialization happens, whether the
rejection enum stays protocol-free, and whether custom validation shapes
(decorators) survive. Proposal on the table:
`RequestRejection::ConstraintViolation(Box<dyn HttpModeledError + Send>)`.
This doc records every alternative considered and why each loses.

---

## 0 — Legacy status quo: pre-serialized per-codec bytes

What ships today: the rejection carries the **already-serialized body**, typed
per codec —

```rust
// rest_json_1/rejection.rs:166
ConstraintViolation(String),
// rpc_v2_cbor/rejection.rs:37 — "Unlike the other protocols, RPC v2 uses CBOR,
// a binary serialization format, so we take in a `Vec<u8>` here instead"
ConstraintViolation(Vec<u8>),
```

filled by a **generated, per-protocol** `From<ConstraintViolation> for
RequestRejection` that calls the per-protocol payload serializer at
construction time.

Why it fails (all observed, not hypothetical):

- **It is the reason per-protocol rejection enums exist.** Every other variant
  of `RequestRejection` is protocol-generic; only this one's payload type
  differs per codec. One variant forces N enum copies.
- **Serialization happens away from the boundary.** The body is fixed at
  rejection-construction; status/headers/discriminator get assembled elsewhere
  and later. Two half-serializations instead of one seam.
- **Multiprotocol collision.** The conversion impls render outside the
  per-protocol module transformer, so a two-protocol crate materializes both
  serializer copies at one shared path — the pokemon E0308 (`String` vs
  `Vec<u8>`) recorded in `specs/handoff.md`.
- **Plumbing tax.** Everything built last session to keep this alive —
  `serverValidationExceptionErrorSerializer`, RuntimeType materialization to
  force the serializer to exist in flag-on crates, the multi-protocol raw-path
  branch — services only this variant.

## 1 — Carry the concrete typed ValidationException value

The obvious "just keep the value" designs. The blocker in both sub-variants is
**nameability**: the runtime crate must declare the variant's type, but the
validation shape is *crate-local generated code* — either
`smithy.framework#ValidationException` or a **custom shape** chosen by one of
the three validation decorators (`SmithyValidationExceptionDecorator`,
`CustomValidationExceptionWithReasonDecorator`,
`UserProvidedValidationExceptionDecorator`).

- **(a) `RequestRejection<V>`** — make the enum generic over the validation
  shape. The parameter infects every signature that touches rejections:
  `FromRequest`, `Upgrade`, `IntoResponse` for the rejection, every
  `From<...> for RequestRejection` conversion, user-visible bounds. A
  whole-crate generic to route one variant's payload.
- **(b) runtime-owned concrete struct** — hardcode a `ValidationException
  { message, field_list }` in `aws-smithy-http-server`. Either custom shapes
  can't be represented (reason codes, renamed/extra members — the entire point
  of two of the three decorators) or the decorator mechanism dies. Also
  freezes the shape's wire form into a runtime semver surface.

## 2 — Carry the unconverted ConstraintViolation, convert at the boundary

Defer the `ConstraintViolation → ValidationException` conversion too.
`ConstraintViolation` is itself a **per-operation-input generated enum**
(`crate::operation::.../ConstraintViolation`) — the same nameability problem
as option 1, so it must also hide behind a runtime dyn trait, e.g.
`trait AnyConstraintViolation { fn to_modeled_error(&self) -> Box<dyn
HttpModeledError> }`. Net effect: one extra trait, dyn-boxing moved one layer
earlier, and the boundary now performs conversion *and* serialization. The
conversion is protocol-free — there is nothing boundary-specific about it —
so deferring it buys nothing and costs a trait.

## 3 — Serialize eagerly under the statically-known `P`; carry `http::Response<BoxBody>`

The strongest alternative — treat it fairly. `FromRequest<P>` knows `P` at the
violation site, so it *can* call `P::serialize_error(&err)` right there and
the rejection carries the finished response. Serialization still happens
exactly once; no dyn, no new bounds; the rejection enum unifies.

Why it still loses:

- **Display/logging breaks.** `RequestRejection` derives
  `#[derive(Debug, Error)]` and `Upgrade` logs rejections via
  `tracing::trace!(error = %err, "parameter for the handler cannot be
  constructed")` (`upgrade.rs:189`). An `http::Response<BoxBody>` cannot
  render the violation message — the body is opaque boxed bytes by then. The
  restJson1 variant today interpolates the message (`{0}` on the `String`);
  this option regresses observability structurally, not incidentally.
- **Semantic value destroyed early.** Between `FromRequest` and the boundary
  sit plugins, instrumentation, and (later phases) anything that wants to
  inspect, count, or wrap the modeled error. A response is a dead end; a
  value is inspectable until the last moment.
- **Construction sites couple to response assembly.** Every place a rejection
  is built must be able to build a full response — and the
  `From<ConstraintViolation>` conversion stops being a plain protocol-free
  impl (it needs `P` in scope), pushing protocol type parameters back into
  builder-adjacent generated code.
- **Asymmetric with handler errors.** Handler-returned errors serialize at
  `IntoResponse` time; framework errors would serialize at construction time.
  Two moments instead of one seam.

## 4 — Protocol-neutral intermediate representation (`Document`)

Convert the violation to `aws_smithy_types::Document` (+ schema ref), carry
that, serialize the document at the boundary. Out of scope by construction:
documents-at-request-time is **P2 dynamic machinery**, explicitly excluded by
plan Phase-1 non-goals. It also abandons the static `SerializableStruct` walk
for an allocation-heavy value-tree detour and still needs a schema reference
riding alongside — strictly more machinery than boxing the value itself, for
zero additional capability in this phase.

## 5 — Erased-protocol closure / responder

The rejection carries `Box<dyn FnOnce(???) -> http::Response<BoxBody>>` —
a responder invoked with the protocol at the boundary. But the closure's
argument must be protocol-erased, and `ServerProtocol` is **deliberately not
object-safe** (associated `Codec`, generic `deserialize_request`); a
`dyn`-able mirror trait would have to exist just for this. Alternatively the
closure captures `P` eagerly — which is option 3 wearing a costume. Either
way: more machinery than `Box<dyn HttpModeledError>`, same or worse
properties.

---

## Chosen design (proposed)

```rust
// runtime, one variant, all protocols:
ConstraintViolation(Box<dyn HttpModeledError + Send>),
```

- One **generated, protocol-free** `From<ConstraintViolation> for
  {ValidationShape}` builds the value (frozen message text, fieldList). The
  three decorators customize THIS conversion only.
- Serialization happens **once**, at the same seam handler errors use: the
  rejection's `IntoResponse<P>` calls `P::serialize_error(&*err)`
  (`serialize_error` takes `&dyn HttpModeledError` by design).

| Axis | 0 bytes | 1 typed | 2 raw CV | 3 eager resp | 4 Document | 5 responder | **dyn HttpModeledError** |
|---|---|---|---|---|---|---|---|
| Nameable from runtime crate | yes (String/Vec) | **no** | **no** (needs dyn anyway) | yes | yes | yes | **yes** |
| Custom shapes / decorators | yes (per-proto ser) | (b) **no** | yes | yes | awkward | yes | **yes** |
| One value serves any P | **no** (per-codec type) | yes | yes | **no** (P baked) | yes | (3-like) | **yes** |
| Display/logging of violation | yes | yes | yes | **no** | yes | **no** | **yes*** |
| Serialize once, at the shared seam | **no** | yes | yes | once but early | yes | yes | **yes** |
| Phase-1 scope | yes | yes | yes | yes | **no (P2)** | yes | **yes** |
| Extra machinery | plumbing tax | generics / frozen shape | +1 trait | — | doc walkers | dyn-mirror trait | box + Send |
| Deletion payoff (unify rejection enums, delete validation-serializer plumbing) | — | partial | partial | partial | partial | partial | **full** |

\* subject to the bound gap below.

**Requirements the chosen design imposes — one real gap found.**
`HttpModeledError` today is `HttpModeledError: ModeledError:
SerializableStruct` (`modeled_error.rs:41-57`) with **no `Debug`, no
`Display`, no `Send`/`Sync` bounds**. The variant needs:

- **`Send`** — declared in the box type; generated impls are plain data, so
  adding it is free, but it must be stated (either in the variant or as a
  supertrait).
- **`Debug`** — `RequestRejection` derives `Debug`; a `Box<dyn
  HttpModeledError>` payload doesn't satisfy the derive today. Fix: `Debug`
  supertrait (generated error shapes already derive `Debug`) or a manual
  `Debug` impl for the variant.
- **`Display`** — the `#[error(...)]` message: either follow the CBOR
  precedent (static message, no `{0}` interpolation — accepting the
  restJson1 log-fidelity regression), or add a `Display`/`message()`
  supertrait requirement so logs keep the violation text. Recommend the
  supertrait: generated `@error` shapes already have natural Display
  (they implement `std::error::Error` in generated code).

None of these are design problems — generated error types satisfy all three
trivially — but the trait bounds must be added deliberately at Checkpoint 3,
not discovered by compile error.

---

**Status: Proposed as plan §2d; awaiting explicit sign-off.**
