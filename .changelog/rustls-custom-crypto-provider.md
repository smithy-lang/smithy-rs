applies_to: ["client"]
authors: ["jcdyer"]
references: ["smithy-rs#4662"]
breaking: false
new_feature: true
bug_fix: false
---

# Add `tls::rustls::CryptoMode::Custom(rustls::crypto::CryptoProvider)` to allow custom TLS handling

This enables custom tls handling through the mechanisms enabled by rustls, including support for custom
providers like `rustls-openssl`.
