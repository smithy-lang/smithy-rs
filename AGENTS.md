# Smithy-rs AI Agent Guide

## Package Layout

- **`codegen-core/`** - Shared codegen
- **`codegen-server/`** - Server codegen
- **`codegen-client/`** - Client codegen
- **`rust-runtime/`** - Runtime libraries
- **`codegen-server-test/`** - Server integration tests

Protocol files: `codegen-{core,server}/.../protocols/`

## preludeScope: Rust Prelude Types

**Always use `preludeScope` for Rust prelude types:**

```kotlin
rustTemplate(
    "let result: #{Result}<#{String}, #{Error}> = #{Ok}(value);",
    *preludeScope,  // Provides Result, String, Ok
    "Error" to myErrorType
)
```

❌ Wrong: `"let result: Result<String, Error> = Ok(value);"`
✅ Correct: Use `*preludeScope` in templates

## RuntimeType and Dependencies

`RuntimeType` objects contain:
- **`path`**: Rust path (e.g., `"::mime::Mime"`)
- **`dependency`**: `CargoDependency` or `InlineDependency`

Using a `RuntimeType` automatically adds its dependency to `Cargo.toml`.

### Creating RuntimeTypes

```kotlin
// Pre-defined dependencies
val Mime = CargoDependency.Mime.toType()
val Bytes = CargoDependency.Bytes.toType().resolve("Bytes")

// Runtime crates
val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
```

### Always Use Symbols

❌ Wrong: `rust("const MIME: ::mime::Mime = ::mime::APPLICATION_JSON;")`
✅ Correct: `rustTemplate("const MIME: #{Mime}::Mime = #{Mime}::APPLICATION_JSON;", "Mime" to RuntimeType.Mime)`

## RuntimeType.forInlineFun: Lazy Generation

Code is only generated if used. `forInlineFun` enables lazy generation:

```kotlin
val mimeType = RuntimeType.forInlineFun("APPLICATION_JSON", module) {
    rustTemplate(
        "pub const APPLICATION_JSON: #{Mime}::Mime = #{Mime}::APPLICATION_JSON;",
        "Mime" to RuntimeType.Mime
    )
}
```

⚠️ **Footgun**: Name collisions mean only one implementation gets generated.

## Testing

### Integration Tests
Test actual generated code, not just codegen logic:

```kotlin
serverIntegrationTest(model) { codegenContext, rustCrate ->
    rustCrate.testModule {
        tokioTest("test_accept_header") {
            rustTemplate("""
                let request = ::http::Request::builder()
                    .header("Accept", "application/cbor")
                    .body(Body::empty()).unwrap();
                let result = MyInput::from_request(request).await;
                result.expect("should accept valid header");
            """)
        }
    }
}
```

### Running Tests

**Codegen tests:**
```bash
./gradlew test --tests "*MyTest*"
./gradlew codegen-server-test:assemble --quiet
```

**Debug failing tests:**
```bash
# Remove --quiet to see failure details
./gradlew :codegen-core:test --tests "*InlineDependencyTest*"
# Extract just the error from HTML report (avoid HTML markup pollution)
grep -A 5 "AssertionError\|Exception" codegen-core/build/reports/tests/test/classes/software.amazon.smithy.rust.codegen.core.rustlang.InlineDependencyTest.html
```

**Runtime tests:**
```bash
cd rust-runtime && cargo test --quiet -p aws-smithy-types
```

**Protocol tests:**
```bash
./gradlew codegen-client-test:assemble --quiet
cd codegen-client-test/build/smithyprojections/codegen-client-test/rest_xml_extras/rust-client-codegen
cargo test --quiet
```

## Viewing Generated Code

Generated code appears in:
```
codegen-server-test/build/smithyprojections/codegen-server-test/SERVICE_NAME/rust-server-codegen/
```

Enable debug comments:
```kotlin
serverIntegrationTest(model, IntegrationTestParams(
    additionalSettings = ServerAdditionalSettings.builder()
        .generateCodegenComments()  // Adds Kotlin source line comments
        .toObjectNode()
)) { /* test code */ }
```
