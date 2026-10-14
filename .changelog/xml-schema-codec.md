---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: true
---

Fix schema-based XML decoding of nested and recursive collections and use tokenizer
boundaries to extract elements correctly around comments, CDATA, processing
instructions, and quoted attributes. Flattened collections now retain the configured
recursion-depth limit. Add independent document and root-name validation settings,
opt-in filtering of wrapped collection element names, and a modeled-error root-name
override. Default XML recovery behavior is preserved.
