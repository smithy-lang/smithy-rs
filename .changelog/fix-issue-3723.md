---
applies_to:
- server
authors:
- lee-1104
references:
- smithy-rs#3723
breaking: false
new_feature: false
bug_fix: true
---
Validate body contents when there is empty or no operation input. 
For JSON protocols, an empty body or `{}` is accepted. 
For CBOR, an empty body or an empty CBOR map (`0xA0`) is accepted. 
For XML, only an empty body is accepted. This fixes the `AdditionalTokensEmptyStruct` malformed request protocol test for RPC v2 CBOR.
