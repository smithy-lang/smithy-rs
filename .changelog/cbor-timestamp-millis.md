---
applies_to: ["client", "server"]
authors: ["franco2002lu"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Fix RPC v2 CBOR timestamp decoding losing a millisecond. A float timestamp that is the closest double to a whole number of milliseconds, such as `1700000000.123`, was truncated to the millisecond below about half the time. It now decodes as that millisecond. Timestamps with sub-millisecond precision are still truncated to milliseconds.
