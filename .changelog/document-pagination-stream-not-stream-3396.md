# Document why PaginationStream isn't a futures Stream
---
applies_to: ["client", "aws-sdk-rust"]
authors: ["hippi345"]
references: ["smithy-rs#3396"]
breaking: false
new_feature: false
bug_fix: false
---
Add documentation to `PaginationStream` explaining why it does not implement the `futures::stream::Stream` trait (0.x dependency constraint) and linking to `PaginationStreamExt` for users who need Stream trait compatibility.
