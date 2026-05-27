---
applies_to:
- server
authors:
- rcoh
references:
- smithy-rs#4640
breaking: false
new_feature: true
bug_fix: false
---

Add `requestBodyMaxBytes` codegen setting to limit the size of non-streaming request bodies buffered into memory. When set, requests exceeding the limit are rejected with a 400 response before additional data is read. This prevents memory-exhaustion denial-of-service attacks via unbounded `Transfer-Encoding: chunked` bodies. The default is `0` (no limit) for backwards compatibility. Streaming operations are unaffected.

To enable, add to your `smithy-build.json`:
```json
{
  "codegen": {
    "requestBodyMaxBytes": 2097152
  }
}
```
