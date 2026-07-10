---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: []
breaking: true
new_feature: true
bug_fix: false
---
Updated `aws_smithy_types::Document` to cover the full Smithy data model in line with recent updates to the Smithy "Document Types and Type Registries" specification. The enum gains four variants — `Blob(Vec<u8>)`, `Timestamp(DateTime)`, `BigInteger(BigInteger)`, and `BigDecimal(BigDecimal)` — and is now marked `#[non_exhaustive]` so future Smithy data-model extensions can ship as additive changes. Three companion types join the public API at `aws_smithy_types::*`: `DiscriminatedDocument` (wraps a `Document` with an optional shape-ID discriminator and protocol-aware codec settings, used by the type-registry deserialization flow); `DocumentSettings` (trait for format-specific coercion, e.g. base64-decoding JSON strings to blobs); and `DocumentError` (numeric-coercion overflow, type mismatch, and invalid-input errors emitted by the new numeric / blob / timestamp accessors).

Construction via the existing variants (`Document::String("...".into())`, `Document::Object(map)`, etc.) and the existing `From<...>` impls (`bool`, `&str`, `u64`, etc.) continue to work unchanged.

**Migration recipe**

Exhaustive `match` statements on `Document` no longer compile. Because the enum is now `#[non_exhaustive]`, external code must include a wildcard arm even if every currently-known variant is named — this is what protects future variant additions from being a breaking change. Add a `_ =>` arm:

```rust
match doc {
    Document::Null => /* ... */,
    Document::Bool(b) => /* ... */,
    Document::Number(n) => /* ... */,
    Document::String(s) => /* ... */,
    Document::Object(o) => /* ... */,
    Document::Array(a) => /* ... */,
    // Optionally handle the new variants explicitly:
    Document::Blob(b) => /* base64-encode? */,
    Document::Timestamp(ts) => /* format? */,
    Document::BigInteger(bi) => /* string-encode? */,
    Document::BigDecimal(bd) => /* string-encode? */,
    // Required by #[non_exhaustive]:
    _ => /* fallback for future variants */,
}
```

`Document::Object` map entries are now iterated in insertion order.

**`Document::Object` inner type change**

`Document::Object` now wraps an insertion-ordered `aws_smithy_types::document::DocumentObject` instead of a `std::collections::HashMap<String, Document>`. This is source-breaking beyond the variant additions in two ways:

- Naming the old inner type no longer compiles. A pattern bind or annotation that referred to `HashMap<String, Document>` (for example `Document::Object(map) => { let _: &HashMap<String, Document> = map; }`) must be updated to `DocumentObject`.
- `HashMap`-only methods are unavailable. `DocumentObject` mirrors most of the `HashMap` surface (`insert`, `get`, `get_mut`, `contains_key`, `remove`, `len`, `is_empty`, `clear`, `iter` / `iter_mut`, `keys`, `values` / `values_mut`, indexing by `&str` / `&String`, and the `IntoIterator` / `FromIterator` / `Extend` impls), but methods such as `.entry()`, `.retain()`, `.drain()`, `.capacity()`, and `.get_key_value()` are not provided.

Migration:

```rust
use aws_smithy_types::document::DocumentObject;
use std::collections::HashMap;

// Build a DocumentObject from a HashMap...
let obj = DocumentObject::from(map);
// ...or from scratch:
let mut obj = DocumentObject::new();
obj.insert("key".to_string(), Document::String("value".to_string()));

// Recover a HashMap when one is specifically required:
let map: HashMap<String, Document> = obj.into_iter().collect();
```
