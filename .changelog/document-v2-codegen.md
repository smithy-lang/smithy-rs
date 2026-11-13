---
applies_to: ["client", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853"]
breaking: false
new_feature: true
bug_fix: false
---

In clients generated with schema serde, structures now have a `DocumentV2` form
of each member that holds `Document` values. For a member
`payload: Option<Document>`, the structure gets a `payload_v2()` getter, and its
builder and the operation's fluent builder get `payload_v2`, `set_payload_v2`,
and `get_payload_v2`. Lists and maps of documents get the same methods with
`DocumentV2` elements. The `Document` field and methods are unchanged. Requests
send the `DocumentV2` form when it is set, and the `Document` field otherwise.
Responses are read into the `DocumentV2` form, which keeps numbers that don't
fit in a `u64`, `i64`, or `f64` exactly, and the order of object members. The
`Document` field holds its `DocumentV2::to_document_lossy` conversion, which is
the value it held before. When the `DocumentV2` form is set on a builder,
`build()` sets the `Document` field the same way, and either form satisfies a
required member. Clearing the `DocumentV2` form with `set_payload_v2(None)` lets
a value set with the `Document` methods take effect again. Equality and `Debug`
output still cover only the `Document` fields. Union variants keep only their
`Document` form. Other clients are unchanged.
