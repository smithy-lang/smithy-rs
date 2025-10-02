# Smithy-rs Codegen Guide for AI Agents

This guide provides essential information for AI agents working on smithy-rs codegen, based on lessons learned from implementing Accept header support for event streaming operations.

## Package Layout

Smithy-rs follows a modular structure:

- **`codegen-core/`** - Core codegen functionality shared between client and server
- **`codegen-server/`** - Server-specific codegen logic
- **`codegen-client/`** - Client-specific codegen logic
- **`rust-runtime/`** - Runtime libraries that generated code depends on
- **`codegen-server-test/`** - Integration tests for server codegen
- **`examples/`** - Example services for testing and demonstration

Key files for protocol work:
- `codegen-core/src/main/kotlin/software/amazon/smithy/rust/codegen/core/smithy/protocols/` - Protocol implementations
- `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/` - Server protocol generators

### The preludeScope: Rust Prelude Types

**Always use `preludeScope` for Rust prelude types** instead of referencing them directly.

The `preludeScope` contains all Rust prelude types (types automatically imported in every Rust module):

```kotlin
// Defined in RuntimeType companion object
val preludeScope by lazy {
    arrayOf(
        // Common prelude types
        "Copy" to std.resolve("marker::Copy"),
        "Clone" to Clone,
        "Option" to Option,
        "Some" to Option.resolve("Some"),
        "None" to Option.resolve("None"),
        "Result" to std.resolve("result::Result"),
        "Ok" to std.resolve("result::Result::Ok"),
        "Err" to std.resolve("result::Result::Err"),
        "String" to String,
        "Vec" to Vec,
        "Into" to Into,
        "From" to From,
        // ... and many more
    )
}
```

**Usage Example:**
```kotlin
rustTemplate(
    """
    let default_retry_partition = match config.region() {
        #{Some}(region) => #{Cow}::from(format!("{default_retry_partition}-{}", region)),
        #{None} => #{Cow}::from(default_retry_partition),
    };
    """,
    *preludeScope,  // Provides Some, None, etc.
    "Cow" to RuntimeType.Cow,
)
```

❌ **Wrong** - Hardcoding prelude types:
```kotlin
rustTemplate(
    "let result: Result<String, Error> = Ok(value);",
    "Error" to myErrorType
)
```

✅ **Correct** - Using preludeScope:
```kotlin
rustTemplate(
    "let result: #{Result}<#{String}, Error> = #{Ok}(value);",
    *preludeScope,  // Provides Result, String, Ok
    "Error" to myErrorType
)
```

**Why use preludeScope?**
- **Consistency**: Ensures all prelude types use the same fully-qualified paths
- **Maintainability**: Single source of truth for prelude type definitions
- **Correctness**: Handles edge cases and Rust edition differences automatically

## Symbol Usage and Dependency Management

### How Dependencies Actually Work

Dependencies in smithy-rs come from `RuntimeType` objects, which contain:
- **`path`**: The fully qualified Rust path (e.g., `"::mime::Mime"`)
- **`dependency`**: A `RustDependency` (either `CargoDependency` or `InlineDependency`)

When a `RuntimeType` is used in generated code, its dependency is automatically added to `Cargo.toml`.

### Creating RuntimeTypes with Dependencies

**From CargoDependency:**
```kotlin
// Pre-defined dependencies in CargoDependency companion object
val Mime = CargoDependency.Mime.toType()  // Creates RuntimeType("::mime", CargoDependency.Mime)
val Bytes = CargoDependency.Bytes.toType().resolve("Bytes")  // RuntimeType("::bytes::Bytes", CargoDependency.Bytes)

// Runtime crates (aws-smithy-* crates)
fun smithyHttp(runtimeConfig: RuntimeConfig) = runtimeConfig.smithyRuntimeCrate("smithy-http")
val SmithyHttp = CargoDependency.smithyHttp(runtimeConfig).toType()
```

**From RuntimeConfig (smithy runtime crates):**
```kotlin
// These automatically handle versioning and path configuration
val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
val smithyHttp = RuntimeType.smithyHttp(runtimeConfig)
```

### Pre-defined RuntimeTypes

Many common types are already defined in `RuntimeType` companion object:

```kotlin
// Standard library (no dependencies)
RuntimeType.String        // "::std::string::String"
RuntimeType.Vec          // "::std::vec::Vec"
RuntimeType.Option       // "::std::option::Option"

// External crates (with CargoDependency)
RuntimeType.Bytes        // "::bytes::Bytes" + bytes dependency
RuntimeType.Http         // "::http" + http dependency
RuntimeType.Serde        // "::serde" + serde dependency
```

### Always Use Symbols, Not Hardcoded Types

❌ **Wrong** - Hardcoded crate references:
```kotlin
rust("const MIME: ::mime::Mime = ::mime::APPLICATION_JSON;")
```

✅ **Correct** - Use RuntimeTypes in templates:
```kotlin
rustTemplate(
    "const MIME: #{Mime}::Mime = #{Mime}::APPLICATION_JSON;",
    "Mime" to RuntimeType.Mime
)
```

### Automatic Dependency Management

When you use a `RuntimeType` with a dependency:
1. **Cargo.toml gets updated** with the dependency automatically
2. **Imports are managed** in the generated Rust code
3. **Versioning is consistent** across the entire codebase
4. **Features and scopes** are handled correctly

Example of how `CargoDependency.Mime` is defined:
```kotlin
// In CargoDependency companion object
val Mime: CargoDependency = CargoDependency("mime", CratesIo("0.3.16"))

// When you use RuntimeType.Mime, it automatically:
// 1. Adds `mime = "0.3.16"` to Cargo.toml
// 2. Allows `#{Mime}` to resolve to `::mime` in templates
```

## RuntimeType.forInlineFun: "Ask for What You Need"

Smithy-rs follows a principle of "ask for what you need" - code is only generated if it's actually used. `RuntimeType.forInlineFun` is a key tool for this:

### Basic Usage

```kotlin
val mimeType = RuntimeType.forInlineFun("APPLICATION_JSON", module) {
    rust("pub const APPLICATION_JSON: ::mime::Mime = ::mime::APPLICATION_JSON;")
}
```

### Real Example from Codebase

```kotlin
// From ClientReExports.kt
fun configReexport(type: RuntimeType): RuntimeType =
    RuntimeType.forInlineFun(type.name, module = ClientRustModule.config) {
        rustTemplate("pub use #{type};", "type" to type)
    }
```

### Advanced Example from Our Implementation

```kotlin
// Generate MIME type constants only when needed
private fun mimeType(type: String): RuntimeType {
    val variableName = type.toSnakeCase().uppercase()
    val typeName = "CONTENT_TYPE_$variableName"
    return RuntimeType.forInlineFun(typeName, RustModule.private("mimes")) {
        rustTemplate(
            """
            pub(crate) static $typeName: std::sync::LazyLock<#{Mime}::Mime> = std::sync::LazyLock::new(|| {
                ${type.dq()}.parse::<#{Mime}::Mime>().expect("BUG: MIME parsing failed, content_type is not valid")
            });
            """,
            *codegenScope,
        )
    }
}
```

**Key Points:**
- Uses `#{Mime}` symbol instead of hardcoding `::mime::Mime`
- When this symbol gets rendered, smithy-rs automatically adds the `mime` dependency to `Cargo.toml`
- The `*codegenScope` provides access to common symbols like `Mime`
- Only generates the constant if the `RuntimeType` is actually used
```

### ⚠️ Important Footgun

**Name Collision Issue**: If two different fully-qualified types map to the same name, only one will be generated.

```kotlin
// DANGEROUS: Both could map to "APPLICATION_JSON"
val json1 = RuntimeType.forInlineFun("APPLICATION_JSON", module) { /* generates JSON constant */ }
val json2 = RuntimeType.forInlineFun("APPLICATION_JSON", module) { /* generates different JSON constant */ }
// Only one will actually be generated!
```

In our Accept header implementation, this could happen if both `"application/json"` and `"application_json"` were MIME types we cared about, since both would map to `APPLICATION_JSON`.

## Testing Generated Code

### Integration Tests

Always test the actual generated code, not just the codegen logic:

```kotlin
@Test
fun acceptHeaderTests() {
    serverIntegrationTest(model) { codegenContext, rustCrate ->
        rustCrate.testModule {
            generateAcceptHeaderTest("application/vnd.amazon.eventstream", false, codegenContext)
            generateAcceptHeaderTest("application/cbor", false, codegenContext)
            generateAcceptHeaderTest("application/invalid", true, codegenContext)
        }
    }
}

private fun RustWriter.generateAcceptHeaderTest(
    acceptHeader: String,
    shouldFail: Boolean,
    codegenContext: CodegenContext
) {
    tokioTest("test_header_${acceptHeader.toSnakeCase()}") {
        rustTemplate("""
            let request = ::http::Request::builder()
                .header("Accept", ${acceptHeader.dq()})
                .body(Body::empty())
                .unwrap();
            let result = MyInput::from_request(request).await;
        """)

        if (shouldFail) {
            rust("result.expect_err(\"should reject invalid header\");")
        } else {
            rust("result.expect(\"should accept valid header\");")
        }
    }
}
```

### Client Integration Tests

Similar pattern for client code:

```kotlin
clientIntegrationTest(model) { codegenContext, rustCrate ->
    rustCrate.testModule {
        // Test client-side behavior
    }
}
```

## Running Tests

### Codegen Tests (Kotlin)

**Run all codegen tests:**
```bash
./gradlew test
```

**Run specific test class:**
```bash
./gradlew codegen-server:test --tests "*EventStreamAcceptHeaderTest*"
./gradlew codegen-client:test --tests "*MySpecificTest*"
```

**Run tests with pattern:**
```bash
./gradlew test --tests "*Accept*"
```

**Debug failing tests:**
```bash
# Run without --quiet to see failure details
./gradlew :codegen-core:test --tests "*InlineDependencyTest*"

# View detailed HTML test report (path shown in error output)
open codegen-core/build/reports/tests/test/index.html

# Use --quiet to suppress verbose output for successful builds
./gradlew codegen-server-test:assemble --quiet
```

### Integration Tests (Generated Rust Code)

**Build and run server integration tests:**
```bash
./gradlew codegen-server-test:clean
./gradlew codegen-server-test:assemble
cd codegen-server-test/build/smithyprojections/codegen-server-test/rpcv2Cbor_extras/rust-server-codegen
cargo test
```

**Run specific Rust test:**
```bash
cd codegen-server-test/build/smithyprojections/codegen-server-test/rpcv2Cbor_extras/rust-server-codegen
cargo test accept_header
```

**Build and run client integration tests:**
```bash
./gradlew codegen-client-test:clean
./gradlew codegen-client-test:assemble
cd codegen-client-test/build/smithyprojections/codegen-client-test/SERVICE_NAME/rust-client-codegen
cargo test
```

### Runtime Tests (Rust)

**Run all runtime tests:**
```bash
cd rust-runtime
cargo test
```

**Run specific runtime crate tests:**
```bash
cd rust-runtime/aws-smithy-http-server
cargo test accept_header
```

**Run with specific features:**
```bash
cd rust-runtime/aws-smithy-http-server
cargo test --features "test-util"
```

### Example Service Tests

**Run Pokemon service tests:**
```bash
cd examples
cargo test
```

**Run specific example test:**
```bash
cd examples/pokemon-service
cargo test event_streaming
```

### Protocol Tests

**Run protocol test generation:**
```bash
./gradlew codegen-server-test:assemble
./gradlew codegen-client-test:assemble
```

**Run generated protocol tests:**
```bash
# Server protocol tests
cd codegen-server-test/build/smithyprojections/codegen-server-test/rpcv2Cbor/rust-server-codegen
cargo test

# Client protocol tests
cd codegen-client-test/build/smithyprojections/codegen-client-test/rpcv2Cbor/rust-client-codegen
cargo test
```

### Debugging Test Failures

**Run with verbose output:**
```bash
./gradlew test --info
cargo test -- --nocapture
```

**Run single test with debug:**
```bash
./gradlew codegen-server:test --tests "*MyTest*" --info
```

**Check generated code location:**
```bash
# Test output shows paths like:
# Generated Rust crate: file:///path/to/smithy-rs/build/tmp/test-12345/rust-server-codegen
# Navigate there to inspect generated code
```

### Common Test Patterns

**Test only compilation (no execution):**
```bash
cd generated-code-directory
cargo check
```

**Test with specific Rust toolchain:**
```bash
cargo +stable test
cargo +nightly test
```

**Clean and rebuild everything:**
```bash
./gradlew clean
./gradlew codegen-server-test:assemble
```

## Viewing Generated Code

### Method 1: Build Output

Run the codegen build to see generated files:

```bash
./gradlew codegen-server-test:assemble
```

Generated code appears in:
```
codegen-server-test/build/smithyprojections/codegen-server-test/rpcv2Cbor_extras/rust-server-codegen/
```

Replace `rpcv2Cbor_extras` with your specific service name.

### Method 2: Unit Test Output

Run any integration test - the output includes links to generated files:

```
Generated Rust crate: file:///path/to/smithy-rs/build/tmp/test-12345/rust-server-codegen
```

You can navigate to these paths to inspect the generated Rust code.

### Method 3: Debug Mode

Enable debug comments in codegen settings:

```kotlin
serverIntegrationTest(
    model,
    IntegrationTestParams(
        additionalSettings = ServerAdditionalSettings.builder()
            .generateCodegenComments()  // Adds Kotlin source line comments
            .toObjectNode()
    )
) { codegenContext, rustCrate ->
    // Your test code
}
```

This adds comments like:
```rust
/* ServerHttpBoundProtocolGenerator.kt:245 */
static CONTENT_TYPE_MYOPERATION: std::sync::LazyLock<::mime::Mime> =
    std::sync::LazyLock::new(|| {
        "application/cbor".parse::<::mime::Mime>()
            .expect("BUG: MIME parsing failed")
    });
```

## Example: Complete Feature Implementation

Here's the pattern we used for Accept header support:

1. **Identify the problem**: Server rejected spec-compliant headers
2. **Add protocol method**: `legacyBackwardsCompatContentType()`
3. **Implement in protocol**: Return legacy MIME type for compatibility
4. **Update codegen**: Generate dual validation logic
5. **Add integration tests**: Test both old and new headers
6. **Verify generated code**: Check that both constants and validation logic are correct

The key insight: Always work backwards from the generated Rust code you want to see, then figure out what Kotlin codegen changes are needed to produce it.
