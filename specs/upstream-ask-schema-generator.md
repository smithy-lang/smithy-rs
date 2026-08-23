# Ask for the #4721 authors: make `SchemaGenerator` extensible for server codegen

## Context

The server side of smithy-rs is adopting schema-decoupled serialization, starting
with error responses: every `@error` shape (and everything reachable from one) gets
`Schema<'static>` statics and a `SerializableStruct` impl, and a new
`ServerProtocol::serialize_error` seam in `aws-smithy-http-server` drives the #4721
codecs off them. The output is gated by a byte-identity suite: for every protocol,
the schema-driven error response must match the legacy generated serializers
byte-for-byte (status, headers, body).

We want to reuse `SchemaGenerator` for this — it already emits exactly the statics
and `serialize_members` walkers we need. We cannot: the class is final, every
method and constructor val is `private`, and four server-side requirements have no
seam. Today we maintain a ~1,500-line serialize-only copy
(`codegen-server/.../generators/ServerSchemaGenerator.kt`) that must be manually
re-synced every time the core file changes. This document is the ask to remove
that fork, with the full reasoning per item.

Two ways to satisfy it — either works for us, and **Option B is strictly better**
(it deletes our copy outright; a reference diff implementing all of Option B in
core in ~79 lines exists on our spike branch, commit `3ad0fc895`):

- **Option A**: `open class` + `protected` on the members listed below, so a
  server subclass can exist.
- **Option B**: land the four behaviors in core directly — none of them changes
  client output.

---

## Item 1 — `renderMemberSchemas`, `renderSchemaStatic`, `renderSerializableStruct`, `renderSerializableUnion`

**Ask (A)**: `protected`. **Ask (B)**: add a public `renderSerializeOnly()`
composing them.

**Why the server can't call `render()` as-is.** `render()` unconditionally emits
the deserialization surface — `renderDeserializeMethod`,
`renderDeserializeHttpHeaders`, `renderDeserializeUnion` — and that surface is
client-shaped in three load-bearing ways:

1. **`Self::builder()` and direct builder-field access.** The generated
   `deserialize()` does `let mut builder = Self::builder();` and, in its
   error-correction block, *assigns builder fields directly*
   (`builder.message = builder.message.or(Some(String::new()))`). Server builders
   are a different animal: under `publicConstrainedTypes=false` there are two
   builders per shape (public `build_enforcing_required_and_enum_traits` vs
   internal `build_enforcing_all_constraints` in a `pub(crate) *_internal`
   module), and field visibility/paths don't match what the generated code
   assumes. The emitted code simply does not compile against server builders.

2. **Client "error correction" is semantically wrong on a server.** The
   deserialize method's fallback block *defaults missing required members*
   (empty string, `0`, epoch timestamp…) so a lenient client can tolerate a
   misbehaving service. A server must do the opposite: a missing `@required`
   member is a constraint violation that becomes a 400
   `ValidationException` on the wire. Generating code that silently
   manufactures required values inside a server crate is a correctness hazard
   even if it never runs — and it *would* run if anything wired it up.

3. **The server doesn't need shape-level deserialization at all in this phase.**
   Errors are serialize-only (servers emit them, never parse them), and request
   deserialization stays on the existing validated `FromRequest` pipeline.
   Dead deserialize code on every error shape is pure crate bloat under the
   generated workspaces' `-D warnings` regime.

The four requested methods are exactly the serialize-side building blocks:
member-schema statics, the shape schema static, and the two `serialize_members`
impls. With them `protected`, our whole subclass is ~30 lines. With Option B,
`renderSerializeOnly()` is a pure recomposition of existing private pieces —
no behavior change to any existing caller.

---

## Item 2 — the string arms of `writeMethodForShape` / `unionVariantWriteExpr`

**Ask (A)**: `protected open` (or a small `protected open fun stringWriteExpr`
hook). **Ask (B)**: change the string arm in core to:

```kotlin
is StringShape ->
    if (isStringEnum(target) || symbolProvider.toSymbol(member).rustType()
            .stripOuter<RustType.Option>() != RustType.String) {
        "ser.write_string(&$ref, val.as_str())?;"
    } else {
        "ser.write_string(&$ref, val)?;"
    }
```

**Why.** The generator assumes a member targeting a `StringShape` has Rust type
`String`. That holds for every client symbol provider, but server codegen under
`publicConstrainedTypes=true` (the default) wraps constrained, input-reachable
strings in newtypes: `@length(max: 256) string ErrorMessage` generates
`pub struct ErrorMessage(String)`, and any member targeting it — including
members of `@error` shapes — has that newtype as its field type. The current
emission then fails to compile:

```text
error[E0308]: mismatched types
   --> ebs/.../error.rs:853
853 |             ser.write_string(&INTERNALSERVEREXCEPTION_MEMBER_MESSAGE, val)?;
    |                 ------------ expected `&str`, found `&ErrorMessage`
```

(Concrete reproduction: the `ebs` model in `codegen-server-test` — its error
shapes' `message` members target `ErrorMessage`, a `@length`-constrained string
also reachable from operation input. Seven E0308s.)

Every server constrained-string newtype exposes `pub fn as_str(&self) -> &str`,
so the fix is one branch. The symbol-provider check makes it a **no-op for
clients** (their string members are always `String`), so Option B changes no
client output while making the shared core class usable against any symbol
provider — arguably fixing a latent portability assumption rather than adding a
server feature. The same arm exists in `unionVariantWriteExpr` (union variants
can hold constrained newtypes too); it needs the member passed in, which is
available at its single call site.

---

## Item 3 — the union `Unknown` arm (straight bug in shared core)

**Ask (B only — this one should just be fixed)**: in `renderSerializableUnion`,
gate the fallback arm on the target:

```kotlin
if (codegenContext.target == CodegenTarget.CLIENT) {
    rustTemplate("Self::${UnionGenerator.UNKNOWN_VARIANT_NAME} => return Err(...)")
}
```

**Why.** The arm `Self::Unknown => return Err(SerdeError::custom(...))` is
emitted unconditionally, but the `Unknown` variant only exists on client unions.
Server unions are generated with `renderUnknownVariant = false` (by design:
a server must reject unknown variants during request deserialization rather than
represent them), so the emitted match arm refers to a variant that does not
exist — E0599 on every server union. `SchemaGenerator` lives in `codegen-core`,
which is shared by both targets; a core generator that hard-codes a
client-only variant is a bug regardless of our project. `codegenContext.target`
is already a constructor input, so the gate is one line.

---

## Item 4 — constructor state: `codegenContext`, `writer`, `shape`, and derived `model`, `symbolProvider`, `smithySchema`

**Ask (A)**: `protected val`. (Moot under Option B.)

**Why each one specifically** — any overriding/composing subclass needs them:

- **`writer`** — `SchemaGenerator` takes its `RustWriter` at construction, not
  per render call, so a subclass composing its own render entry point has no way
  to emit anything without it.
- **`shape`** — every render decision keys off it (structure vs union, member
  iteration, shape id for the statics).
- **`symbolProvider`** — needed by the item-2 override (resolving a member's
  actual Rust type is the only reliable way to detect a constrained newtype;
  trait-sniffing the model is wrong because newtypes are only generated for
  *input-reachable* constrained shapes).
- **`model`** — resolving member targets (`model.expectShape(member.target)`)
  in any overridden write-expr logic.
- **`smithySchema`** — the `RuntimeType` for `aws-smithy-schema`; every emitted
  path (`Schema`, `ShapeId`, `ShapeType`, `serde::*`) resolves through it, and
  reconstructing it from `runtimeConfig` in a subclass duplicates state that
  already exists.
- **`codegenContext`** — source of `target` (item 3) and `runtimeConfig`;
  also what a subclass passes to any shared helpers.

---

## Item 5 (completes the picture) — member write order: `serializeMemberOrder`

Not in the original visibility table, but without it a subclass must override
`renderSerializableStruct` wholesale, so it belongs in the ask.

**Ask (B, preferred)**: an optional constructor parameter,

```kotlin
private val serializeMemberOrder: List<MemberShape>? = null,
```

used as `val members = serializeMemberOrder ?: shape.allMembers.values.toList()`
in `renderSerializableStruct`. Default `null` preserves current behavior
exactly; statics, indices, and the schema member array are unaffected — only
the order of write calls in `serialize_members`.

**Why.** Our merge gate is byte-identity with the legacy generated serializers,
and we found the legacy member order is **protocol-dependent**:

- REST protocols (`restJson1`, `restXml`) serialize error document members in
  **member-name-sorted order** — `HttpTraitHttpBindingResolver.mappedBindings`
  ends in `.sortedBy { it.memberName }` (`HttpBindingResolver.kt:226`), and the
  error serializer takes its member list from `errorResponseBindings`. Observed:
  `ValidationException` writes `fieldList` before `message`; restJson1's
  `ComplexError` writes `Nested` before `TopLevel`.
- RPC protocols (`awsJson 1.0/1.1`, `rpcv2Cbor`) use `StaticHttpBindingResolver`,
  which binds `shape.members()` verbatim — **model order**. Observed: awsJson1.1's
  `ComplexError` writes `TopLevel` before `Nested` (opposite of restJson1, same
  shape name, different model).

`SchemaGenerator` today always iterates `allMembers` (model order), which is
byte-identical for RPC but not for REST error bodies. The parameter lets the
caller supply whatever order its compatibility contract requires — a generic
capability, not a server-specific one (any consumer with an ordering contract,
e.g. canonicalization or golden-file stability, can use it).

---

## What you get for taking Option B

- We delete `ServerSchemaGenerator.kt` (~1,500 lines of drift-prone copy) and
  call core directly.
- Core gains: a serialize-only entry point, correct behavior against non-client
  symbol providers, a fixed server-union bug, and an ordering knob — all
  default-off / client-invisible.
- Reference implementation: commit `3ad0fc895` on our spike branch contains the
  exact core diff (+79/−4 lines), already validated by a 47-test wire suite
  including 10 legacy-vs-schema byte-identity goldens across restJson1,
  awsJson 1.0/1.1, and rpcv2Cbor.
