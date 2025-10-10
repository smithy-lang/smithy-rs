---
applies_to: ["client", "server"]
authors: ["haydenbaker", "nated0g"]
references: ["smithy-rs#4338"]
breaking: false
new_feature: false
bug_fix: true
---
Fix builder accessor methods to use symbol provider for naming. This resolves conflicts when struct members are renamed due to reserved words (e.g., `meta` -> `meta_value`), ensuring setter and getter methods use the correct renamed field names.
