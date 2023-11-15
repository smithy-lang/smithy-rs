<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Behavior Versions
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: client

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC describes "Behavior Versions," a mechanism to allow SDKs to ship breaking behavioral changes like a new retry strategy, while allowing customers who rely on extremely consistent behavior to evolve at their own pace.

By adding behavior major versions (BMV) to the Rust SDK, we will make it possible to ship new secure/recommended defaults to new customers without impacting legacy customers.

The fundamental issue stems around our inability to communicate and decouple releases of service updates and behavior within a single major version.

Both legacy and new SDKs have the need to alter their SDKs default. Historically, this caused new customers on legacy SDKs to be subject to legacy defaults, even when a better alternative existed.

For new SDKs, a GA cutline presents difficult choices around timeline and features that can’t be added later without altering behavior.

Both of these use cases are addressed by Behavior Versions.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

In the current version of the SDK, users can construct clients without indicating any sort of behavior major version.
Once this RFC is implemented, there will be two ways to set a behavior major version:

1. In code via `aws_config::defaults(BehaviorVersion::latest())` and `<service>::Config::builder().behavior_version(...)`. This will also work for `config_override`.
2. By enabling `behavior-version-latest` in either `aws-config` (which brings back `from_env`) OR a specific generated SDK crate

```toml
# Cargo.toml
[dependencies]
aws-config = { version = "1", features = ["behavior-version-latest"] }
# OR
aws-sdk-s3 = { version = "1", features = ["behavior-version-latest"] }
```

If no `BehaviorVersion` is set, the client will panic during construction.

`BehaviorVersion` is an opaque struct with initializers like `::latest()`, `::v2023_11_09()`. Downstream code can check the version by calling methods like `::supports_v1()`

When new BMV are added, the previous version constructor will be marked as `deprecated`. This serves as a mechanism to alert customers that a new BMV exists to allow them to upgrade.

How to actually implement this RFC
----------------------------------

In order to implement this feature, we need to create a `BehaviorVersion` struct, add config options to `SdkConfig` and `aws-config`, and wire it throughout the stack.
```rust
/// Behavior major-version of the client
///
/// Over time, new best-practice behaviors are introduced. However, these behaviors might not be backwards
/// compatible. For example, a change which introduces new default timeouts or a new retry-mode for
/// all operations might be the ideal behavior but could break existing applications.
#[derive(Debug, Clone)]
pub struct BehaviorVersion {
    // currently there is only 1 MV so we don't actually need anything in here.
}
```

To help customers migrate, we are including `from_env` hooks that set `behavior-version-latest` that are _deprecated_. This allows customers to see that they are missing the required cargo feature and add it to remove the deprecation warning.

Internally, `BehaviorVersion` will become an additional field on `<client>::Config`. It is _not_ ever stored in the `ConfigBag` or in `RuntimePlugins`.

When constructing the set of "default runtime plugins," the default runtime plugin parameters will be passed the `BehaviorVersion`. This will select the correct runtime plugin. Logging will clearly indicate which plugin was selected.

Design Alternatives Considered
------------------------------

An original design was also considered that made BMV optional and relied on documentation to steer customers in the right direction. This was
deemed too weak of a mechanism to ensure that customers aren't broken by unexpected changes.

Changes checklist
-----------------

- [x] Create `BehaviorVersion` and the BMV runtime plugin
- [x] Add BMV as a required runtime component
- [x] Wire up setters throughout the stack
- [x] Add tests of BMV (set via aws-config, cargo features & code params)
- [x] ~Remove `aws_config::from_env` deprecation stand-ins~ We decided to persist these deprecations
- [x] Update generated usage examples
