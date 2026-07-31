---
applies_to: ["client", "server"]
authors: ["haydenbaker", "nated0g"]
references: ["smithy-rs#4338"]
breaking: false
new_feature: false
bug_fix: true
---
Fix a duplicate `set_meta` definition when an error structure has a `meta` member. The generated struct field is already renamed to `meta_value`, so the builder's setter and getter now match (`set_meta_value`, `get_meta_value`, `meta_value`). Setters for other renamed members (e.g. `default`, `build`, `builder`) are unchanged.
