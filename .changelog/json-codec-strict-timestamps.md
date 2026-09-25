---
applies_to: ["client", "server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
`JsonCodecSettings` gained `strict_timestamp_formats`. When enabled, the schema-based JSON deserializer requires a timestamp to use exactly the wire form its `@timestampFormat` (or the codec default) prescribes: a number for `epoch-seconds`, an RFC 3339 string without a UTC offset for `date-time`, an IMF-fixdate string for `http-date`. Servers enable it so that the restJson1 `MalformedTimestampBody*` protocol tests are honored; the default remains the tolerant client behavior.
