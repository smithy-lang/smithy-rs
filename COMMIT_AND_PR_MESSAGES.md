# Commit and Pull Request Messages

## Commit Message
```
Complete TDD verification for union unused variable warnings

- Fix unused `inner` variables in empty struct union match arms
- Apply `_inner` prefix solution to JSON, Query, and CBOR serializers
- Add comprehensive regression tests based on S3Control ObjectEncryptionFilter pattern
- Verify fix eliminates unused variable warnings through TDD methodology

Fixes #4308
```

## Pull Request Message

### Fix unused variable warnings in union serialization for empty structures

**Background:**
Fixes unused variable warnings that occur when serializing unions with empty struct members (e.g., S3Control `ObjectEncryptionFilter` pattern). In CI, unused variable warnings are treated as errors, causing build failures.

**Problem:**
```rust
// Generated code before fix:
match input {
    ObjectEncryptionFilter::Sses3(inner) => {}  // ← unused variable warning!
    ObjectEncryptionFilter::Data(inner) => { /* inner used normally */ }
}
```

**Solution:**
```rust  
// Generated code after fix:
match input {
    ObjectEncryptionFilter::Sses3(_inner) => {}  // ← prefixed, no warning!
    ObjectEncryptionFilter::Data(inner) => { /* inner used normally */ }
}
```

**TDD Verification:**
Following Test-Driven Development approach:
1. **Created S3Control test model** - Union with empty struct pattern
2. **Demonstrated failing state** - Found exact unused variable warning
3. **Applied fix consistently** - `_inner` prefix for empty struct cases
4. **Verified success** - Unused variable warnings eliminated

**Changes Made:**
- **JsonSerializerGenerator.kt**: Added empty struct detection and `_inner` prefixing
- **QuerySerializerGenerator.kt**: Added empty struct detection and `_inner` prefixing  
- **CborSerializerGenerator.kt**: Added empty struct detection and `_inner` prefixing
- **SerializerGeneratorTestUtils.kt**: Created shared S3Control-based test model
- **Test files**: Added regression tests for all three protocols

**Verification Results:**
- ✅ **Before fix**: `warning: unused variable: inner` appears
- ✅ **After fix**: No unused variable warnings appear
- ✅ **All protocols covered**: JSON, Query, CBOR consistency

This resolves the original S3Control ObjectEncryptionFilter issue and prevents similar unused variable warnings in CI.
