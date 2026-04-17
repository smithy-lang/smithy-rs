---
applies_to:
- aws-sdk-rust
- client
authors:
- lnj
references: []
breaking: false
new_feature: false
bug_fix: false
---

Optimized BDD endpoint resolution performance by replacing HashMap-based auth schemes with a typed `EndpointAuthScheme` struct, inlining the BDD evaluation loop, and adding a single-entry endpoint cache. The BDD resolver is now up to 49% faster than the original implementation and outperforms the tree-based resolver on most benchmarks.
