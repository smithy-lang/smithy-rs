---
applies_to: ["client", "aws-sdk-rust"]
authors: ["sachinsharma3191"]
references: ["smithy-rs#4016"]
breaking: false
new_feature: false
bug_fix: true
---
Fix operation metadata using the PascalCased operation struct name instead of the operation name as written in the model. The `Metadata` stored in the config bag now preserves the original casing of the operation name, matching the operation layer name.
