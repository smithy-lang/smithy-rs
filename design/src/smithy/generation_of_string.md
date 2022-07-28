## How a string is generated

### Example model
The smithy model we will use in this page is the following:
```
$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
service SimpleService {
    version: "2022-01-01",
    operations: [
        SimpleOperation,
    ],
}

operation SimpleOperation {
    input: SimpleInput
}

structure SimpleInput {
    serviceId: String
}
```
and the smithy-rs version tag is v0.45.0.

### How the Rust server is generated

The entry point for code generation is a `SmithyBuildPlugin`, in this case the [RustCodegenServerPlugin](https://github.com/awslabs/smithy-rs/blob/c220b03a18a96c9e338ee8e3a89be1ae781486f4/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/RustCodegenServerPlugin.kt#L36).
The `execute()` method takes as parameter a [PluginContext](https://github.com/awslabs/smithy/blob/77a162a30b8da8ee610e3a28bd165e048badb17d/smithy-build/src/main/java/software/amazon/smithy/build/PluginContext.java#L40).
Among other members, the context holds:
```
Model model;
ObjectNode settings;
Set<Path> sources;
```
`model` is the parsed example model as a `Model`, with metadata and all the shapes referenced in it.
`settings` are optional configuration for this build and
