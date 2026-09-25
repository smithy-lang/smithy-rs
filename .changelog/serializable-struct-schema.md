---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["drganjoo"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

`aws_smithy_schema::serde::SerializableStruct` has a new required method,
`fn schema(&self) -> &Schema<'_>`, which returns the schema of the structure or
union itself. Generated structures and unions implement it by returning their
`SCHEMA`. Hand-written implementations must add it. `prelude::UNIT` provides a
schema for `smithy.api#Unit`.
