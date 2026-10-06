---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

A request that no protocol of a schema-routed multi-protocol service claims is now answered the way
Coral answers one: `404 Not Found` with the XML body `<UnknownOperationException/>` and no
`Content-Type` header (previously a bare `400` with an empty body). The body is hardcoded in the
router — it is not produced by restXml, so services that do not serve restXml return it too — and
matches the response of a Coral server whose chain leaves the request to no protocol handler.
