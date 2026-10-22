---
applies_to:
- server
authors:
- fahadzub
references: []
breaking: false
new_feature: true
bug_fix: false
---
The request body size limit can now be configured in `smithy-build.json` at `customizationConfig.protocols.common.requestBodyMaxBytes`, alongside the other common protocol settings. The legacy `codegen.requestBodyMaxBytes` key is still read when the common key is absent; when both are set, the common value takes precedence and a warning is logged.
