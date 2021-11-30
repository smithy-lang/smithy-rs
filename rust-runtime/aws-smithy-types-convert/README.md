# aws-smithy-types-convert

This crate provides utilities for converting between the types defined
in [aws-smithy-types](https://docs.rs/aws-smithy-types) and types commonly used in other libraries.

## Crate Features

By default, no features are enabled. Using the conversions requires enabling one or more features:

```toml
[dependencies]
aws-smithy-types-convert = { version = "VERSION", features = ["convert-chrono"] }
```

Currently, the following conversions are supported:
* `convert-chrono`: Conversions between `DateTime` and [chrono](https://docs.rs/chrono/latest/chrono/).
* `convert-time`: Conversions between `DateTime` and [time](https://docs.rs/time/latest/time/).
