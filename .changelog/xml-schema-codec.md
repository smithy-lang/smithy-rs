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

Preserve list-item XML names for union targets, including renamed and flattened
items, while retaining the target XML name for standalone union serialization.

Defer scalar leaf-text extraction errors until the consumer reads the value.
Transport adapters can skip body copies of HTTP-bound members without
interpreting their scalar contents; reading a scalar with nested elements still
returns an error. Avoid cloning inherited list-item names during union
serialization.
