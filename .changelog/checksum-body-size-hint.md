---
applies_to: ["client", "aws-sdk-rust"]
authors: ["patrick-immometrica"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Response bodies wrapped for checksum validation report the inner body's `size_hint` again, so `ByteStream::size_hint()` on e.g. S3 `GetObject` is no longer unknown.
When the inner body reports its end with the last data frame, the checksum is now verified on that frame and a mismatch is returned in its place, so a consumer that stops reading once it has every byte (such as hyper serving the body with a `Content-Length`) still sees the mismatch.
