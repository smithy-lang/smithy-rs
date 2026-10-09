---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Escape `gen` in generated code. `gen` is a reserved keyword since the Rust 2024
edition, so shape members named `gen` now map to the raw identifier `r#gen`
instead of producing code that fails to compile on edition 2024.
