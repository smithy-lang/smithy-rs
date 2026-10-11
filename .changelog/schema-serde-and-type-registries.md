---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

Added runtime Smithy schemas, protocol-agnostic shape serialization and
deserialization, runtime-selectable client protocols, and generated type and
error registries. `DiscriminatedDocument`, `DocumentSettings`, and
`DocumentError` provide schema-aware document conversion and discriminator-based
type lookup.

The released `aws_smithy_types::Document` API remains source-compatible: it is
still an exhaustively matchable six-variant enum, and `Document::Object` still
contains `HashMap<String, Document>`. Schema-aware document conversion carries
blobs as base64 strings, timestamps as epoch-second numbers, and arbitrary
precision numbers as strings rather than adding variants to `Document`.

Some runtime crates in this release move to a new Cargo compatibility line, so
this is a coordinated transition: the affected crates must be taken together.
Requirements that pin the previous compatibility line are not made compatible.
In particular, a previously published `aws-config` continues to pin the previous
line, and a partial update that upgrades only some of these crates is not
supported. Upgrade the affected crates together.
