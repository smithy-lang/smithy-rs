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
output still cover only the `Document` fields. An ordinary union member that
directly holds documents also gets an additive variant such as
`JsonV2(DocumentV2)`, with `as_json_v2` and `is_json_v2` methods. The original
and V2 variants serialize as the same Smithy member. Responses use the original
`Json(Document)` variant by default. An operation whose response can hold such
a union gets a `document_union_decode_mode` fluent builder method:
`.document_union_decode_mode(DocumentUnionDecodeMode::V2)` makes that
invocation's response, including nested unions, modeled errors, every page of
its paginator, and its event stream events, use the `DocumentV2` variant for
every such member. `DocumentUnionDecodeMode` is re-exported from the client's
`config` module. Other clients are unchanged.
