---
applies_to: ["client"]
authors: ["jasgin"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
`SharedHttpClient` now forwards the public `HttpClient` validation methods to its selector, so external HTTP-client decorators preserve eager connector initialization.
