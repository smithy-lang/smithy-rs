---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["patrick-immometrica"]
references: []
breaking: false
new_feature: false
bug_fix: false
---
`AggregatedBytes::to_vec` copies whole segments into a pre-sized `Vec` instead of collecting byte by byte, which makes `ByteStream::collect().await?.to_vec()` much faster for large bodies.
