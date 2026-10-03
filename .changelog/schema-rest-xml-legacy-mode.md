---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

A schema-routed restXml server reads every child element of a wrapped list or map in a request
body as an item or entry, whatever the element is named, as Coral servers do. Setting
`customizationConfig.protocols."aws.protocols#restXml".legacyMode` to `true` in
`smithy-build.json` makes it read only the children named as the model says, as servers generated
without `schemaSerde` do: `member` (or the member's `@xmlName`) for a list and `entry` for a map,
skipping the others. The codec setting behind it is
`XmlCodecSettingsBuilder::strict_collection_element_names`, off by default, so clients are
unchanged. Flattened lists and maps are unaffected.
