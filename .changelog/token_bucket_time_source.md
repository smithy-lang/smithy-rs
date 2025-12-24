---
applies_to: ["client"]
authors: ["lnj"]
references: ["smithy-rs#4459"]
breaking: false
new_feature: false
bug_fix: true
---

Updated the `TokenBucket` creation to initialize the bucket with the user-provided `TimeSource` from the `Config`.
This fixes the bug in [issue 4459](https://github.com/smithy-lang/smithy-rs/issues/4459) that caused failures
in WASM since the TokenBucket was being created with a default `SystemTime` based `TimeSource`
