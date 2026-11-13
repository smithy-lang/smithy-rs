---
applies_to: ["server"]
authors: ["LucasLeao18"]
references: ["smithy-rs#4556"]
breaking: false
new_feature: false
bug_fix: true
---
Fix Python stub generation emitting Rust raw identifiers (e.g. `r#type`) into `.pyi` files. `stubgen.py` now strips the `r#` prefix from `:param` names so the generated stubs are valid Python.
