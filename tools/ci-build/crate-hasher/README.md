# crate-hasher
The Crate Hasher generates deterministic hashes of crates. This is used as a dependency of `publisher generate-version-manifests` to generate the `source_hash` field:

```toml
[crates.aws-config]
category = 'AwsRuntime'
version = '0.12.0'
source_hash = '12d172094a2576e6f4d00a8ba58276c0d4abc4e241bb75f0d3de8ac3412e8e47'
```

Note: (@rcoh, 4/24/2024): As far as I can tell, no tooling currently relies on the `source_hash`. It seems like this could potentially be used in `runtime-versioner audit` in the future.
