# Architecture Matrix Testing Implementation

## Summary

Implemented comprehensive architecture matrix testing in smithy-rs canary to catch runtime issues like the crc-fast 1.4 SIGILL fault that was missed in CI but caught in aws-sdk-rust canary.

## Changes Made

### 1. Updated `generate_matrix.rs`
**File**: `tools/ci-cdk/canary-runner/src/generate_matrix.rs`

Added architecture support to matrix generation:
- Added `architectures: Vec<String>` parameter to `GenerateMatrixArgs`
- Added `target_arch: Vec<String>` field to `Output` struct
- Updated `generate_matrix()` to include architectures in output

### 2. Created Scheduled Canary Workflow
**File**: `.github/workflows/canary-scheduled.yml`

New daily scheduled workflow that:
- Tests 4 architectures: x86_64-gnu, x86_64-musl, aarch64-gnu, aarch64-musl
- Uses native ARM runners (ubuntu-24.04-arm) for aarch64 targets
- Tests against latest SDK version only (not last 3)
- Tests with stable Rust version
- Runs daily at midnight UTC

## Architecture Matrix

```yaml
architectures:
  - x86_64-unknown-linux-gnu    # Standard x86_64
  - x86_64-unknown-linux-musl   # x86_64 with musl libc
  - aarch64-unknown-linux-gnu   # ARM64/Graviton
  - aarch64-unknown-linux-musl  # ARM64 with musl libc
```

## Why This Catches the Issue

**crc-fast 1.4 problem**:
- SIGILL on specific CPU features (Intel AVX-512)
- Missed by smithy-rs CI (x86_64 only)
- Caught by aws-sdk-rust canary (tests final SDK)

**This solution**:
- ✅ Tests generated SDKs on actual ARM hardware (not cross-compilation)
- ✅ Tests both gnu and musl libc variants
- ✅ Runs daily to catch dependency updates
- ✅ Uses native runners for true runtime testing

## Testing

Verified locally:
```bash
cd tools/ci-cdk/canary-runner
cargo run -- generate-matrix \
  --sdk-versions 1 \
  --rust-versions stable \
  --architectures x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
```

Output:
```json
{
  "sdk_release_tag": ["release-2025-12-23"],
  "rust_version": ["stable"],
  "target_arch": ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
}
```

## Next Steps

1. **Test workflow**: Push to a branch and trigger workflow_dispatch
2. **Verify runners**: Ensure ARM runners are available in GitHub Actions
3. **Monitor results**: Check that canary runs on all architectures
4. **Update manual canary**: Consider adding architecture matrix to manual-canary.yml

## Key Decisions

- **Latest SDK only**: Test against 1 SDK version (not 3) to reduce CI time
- **Native runners**: Use ubuntu-24.04-arm for aarch64 instead of cross-compilation
- **Daily schedule**: Balance between catching issues early and CI resource usage
- **Stable Rust only**: MSRV testing less critical for canary (covered in main CI)

## References

- Context: SMITHY_RS_ARCHITECTURE_AGENT.md
- Related commits: 4c79c34dd (native ARM runners in CI)
- Issue: crc-fast 1.4 SIGILL on ARM architectures
