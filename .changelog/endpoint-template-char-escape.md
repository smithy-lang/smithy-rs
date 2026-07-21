---
applies_to: ["client", "aws-sdk-rust"]
authors: ["lauzmata"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Fix endpoint rules codegen emitting invalid Rust when a templated static segment
is a single character requiring escape in a Rust `char` literal (e.g. `'`, `"`,
`\`). Previously produced `out.push(''');` in `internals.rs`; now always emits
`out.push_str(...)` with proper string escaping.
