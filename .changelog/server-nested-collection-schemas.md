---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Servers using schema serde preserve nested collection member schemas during
deserialization. Nested maps now retain XML key and value names instead of
silently dropping inner entries. Generated readers use the existing member
schema statics for nested lists and maps.
