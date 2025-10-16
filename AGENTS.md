# Smithy-rs AI Agent Guide

## Package Layout

- **`codegen-core/`** - Shared codegen
- **`codegen-server/`** - Server codegen
- **`codegen-client/`** - Client codegen
- **`rust-runtime/`** - Runtime libraries
- **`codegen-server-test/`** - Server integration tests

Protocol files: `codegen-{core,server}/.../protocols/`

## Protocol Tests

Protocol tests validate that generated code correctly implements Smithy protocols (like restJson1, awsJson1_1, etc.).

### Adding Protocol Tests

Protocol tests are defined in Smithy model files using `@httpRequestTests` and `@httpResponseTests` traits:

```
@http(uri: "/my-operation", method: "GET")
@httpRequestTests([
    {
        id: "MyOperationRequest",
        documentation: "Test description",
        protocol: "aws.protocols#restJson1",
        method: "GET",
        uri: "/my-operation",
        queryParams: ["param1=value1", "param2=value2"],
        params: {
            queryMap: {
                "param1": "value1",
                "param2": "value2"
            }
        },
        appliesTo: "client",
    }
])
operation MyOperation {
   input: MyOperationInput,
}
```

### Key Protocol Test Locations

- **`codegen-core/common-test-models/rest-json-extras.smithy`** - restJson1 protocol tests
- **`codegen-core/common-test-models/constraints.smithy`** - Constraint validation tests with restJson1
- **`codegen-client-test/model/main.smithy`** - awsJson1_1 protocol tests

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

### String Interpolation in Templates

**For RuntimeTypes and complex objects**: Use `#{name}` syntax. NOTE: you do not need to use `#{name:W}`. This is now
the default. You may see old code with this pattern.
**For simple strings**: Use `$` with `.dq()` for double-quoted strings

```kotlin
// ❌ Wrong - causes "Invalid type provided to RustSymbolFormatter"
rustTemplate("let content_type = \"#{content_type}\";", "content_type" to "application/json")

// ✅ Correct - use $ interpolation for strings
rustTemplate("let content_type = ${contentType.dq()};")
```

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

## GitHub CLI Integration

**View issues and PRs:**

```bash
gh issue view <number> --repo smithy-lang/smithy-rs
gh pr view <number> --repo smithy-lang/smithy-rs
gh pr diff <number> --repo smithy-lang/smithy-rs
```

**Add comments (use single quotes for complex markdown):**

```bash
gh issue comment <number> --repo smithy-lang/smithy-rs --body 'markdown content with `backticks` and special chars'
```

**Comment Guidelines:**
- Always ask for confirmation before posting comments
- Always start comments with `*Comment from Claude*` in italics

## Investigation Patterns

**Before implementing changes:**

1. **Research existing work** - Check related PRs/issues first
2. **Build and examine generated code** - `./gradlew codegen-server-test:assemble --quiet`
3. **Generated code location** - `codegen-server-test/build/smithyprojections/codegen-server-test/`
4. **Key generated files** - `src/protocol_serde/shape_*.rs`, `src/event_stream_serde.rs`
5. **Look for patterns** - Client vs server codegen often mirrors each other
6. **Identify minimal change** - Understand current behavior before modifying codegen

**Single Protocol Development:**

- When working on a single protocol, uncomment the filter line in `codegen-server-test/build.gradle.kts:111`
- This speeds up builds by only generating code for the protocol you're working on

**Client/Server Symmetry:**

Client changes often show the pattern for server-side implementation

**Configuration Debugging:**
- Server codegen settings go under `"codegen"` not `"codegenConfig"` in smithy-build.json
- When settings aren't working, check the generated smithy-build.json structure first
- Settings placement matters - wrong nesting means settings are ignored silently
- Always verify actual generated configuration matches expectations

**Testing Configuration Settings:**
- Create separate services with different settings to test configuration behavior
- Use integration tests that verify actual generated code behavior, not just compilation
- Test both enabled and disabled states to ensure the setting actually controls behavior

## Testing

### Integration Tests

Test actual generated code, not just codegen logic:

```kotlin
serverIntegrationTest(model) { codegenContext, rustCrate ->
    rustCrate.testModule {
        tokioTest("test_accept_header") {
            rustTemplate(
                """
                let request = ::http::Request::builder()
                    .header("Accept", "application/cbor")
                    .body(Body::empty()).unwrap();
                let result = MyInput::from_request(request).await;
                result.expect("should accept valid header");
            """
            )
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

**Note: Always use `--quiet` with cargo commands to reduce noise and focus on actual errors.**

## Viewing Generated Code

Generated code appears in:

```
codegen-server-test/build/smithyprojections/codegen-server-test/SERVICE_NAME/rust-server-codegen/
```

Enable debug comments:

```kotlin
serverIntegrationTest(
    model, IntegrationTestParams(
        additionalSettings = ServerAdditionalSettings.builder()
            .generateCodegenComments()  // Adds Kotlin source line comments
            .toObjectNode()
    )
) { /* test code */ }
```
