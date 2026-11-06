---
applies_to: ["aws-sdk-rust"]
authors: ["vcjana"]
references: ["smithy-rs#4990"]
breaking: false
new_feature: false
bug_fix: true
---
Response checksums are no longer validated on error responses. An error response that carried a
checksum header was validated as if it were a success, so a mismatch replaced its modeled error with
a body error that was retried as transient. Error responses now keep their modeled errors.
