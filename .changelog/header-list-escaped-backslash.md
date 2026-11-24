---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["patrick-immometrica"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Fix parsing of quoted header list values that end in an escaped backslash, e.g. `"a,\\",b`. The closing quote was treated as escaped, so the list was split in the wrong place or failed to parse.
