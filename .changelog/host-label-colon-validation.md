---
applies_to: ["client", "aws-sdk-rust"]
authors: ["ysaito1001"]
references: ["smithy-rs#4921"]
breaking: false
new_feature: false
bug_fix: true
---

Validate `@hostLabel` members are valid host labels. A `@hostLabel` value goes straight into the
request authority, checked only by `http::uri::Authority::from_str` against the whole expanded
prefix. That parser accepts a colon, and `http::Uri` then reads everything after it as an
unparseable port and discards it. A label containing a colon therefore truncates the resolved host
and drops the service suffix.

The generated code now runs each host label through `is_valid_host_label` before building the
prefix. Accepted labels narrow to `[a-zA-Z0-9-]{1,63}`. A value holding `_` or `.` used to pass and
now fails.
