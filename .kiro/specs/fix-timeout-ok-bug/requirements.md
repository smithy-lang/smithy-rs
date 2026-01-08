# Fix SDK Returning Ok() When Timeout/Error Occurs During Retry

## Problem Statement

The AWS SDK for Rust can incorrectly return `Ok()` (success) when an operation actually failed due to a timeout or other error during the retry loop. This causes silent failures where customers believe their API calls succeeded when they did not.

### Customer Impact

- **Ticket**: P358713763 (Bedrock Custom Model Import team)
- **Symptom**: `DeleteRule` on ALB listener rules returns success, but rules are not deleted
- **Evidence**: No CloudTrail events logged for the "successful" deletions
- **Frequency**: Transient issue (reported 3 times in December 2025)

## Root Cause

In `InterceptorContext::rewind()`, the `output_or_error` field is unconditionally cleared:

```rust
self.output_or_error = None;
```

When an error occurs during the retry loop (timeout, connector error, etc.) and is stored via `ctx.fail()`, the subsequent `rewind()` call clears this error. If no successful response is ever received, the orchestrator may return `Ok()` with no actual result.

## User Stories

### Story 1: Error Preservation During Retry
**As a** developer using the AWS SDK for Rust  
**I want** errors that occur during retry attempts to be preserved  
**So that** I receive accurate error information when my API calls fail

#### Acceptance Criteria
- [ ] When a timeout occurs during retry, the error is preserved and returned to the caller
- [ ] When a connector error occurs during retry, the error is preserved and returned to the caller
- [ ] When any `OrchestratorError` is set via `ctx.fail()`, it survives the `rewind()` operation
- [ ] Successful responses (`Ok`) are still cleared during rewind to allow fresh retry attempts

### Story 2: Accurate Operation Results
**As a** developer using the AWS SDK for Rust  
**I want** operation results to accurately reflect what happened  
**So that** I can trust the SDK's return values and handle failures appropriately

#### Acceptance Criteria
- [ ] Operations that fail due to timeout return an error, not `Ok()`
- [ ] Operations that fail due to connection issues return an error, not `Ok()`
- [ ] Only operations that receive a successful response from AWS return `Ok()`

### Story 3: Debugging Support
**As a** developer debugging SDK issues  
**I want** trace-level logging to show the full request lifecycle  
**So that** I can diagnose issues like silent failures

#### Acceptance Criteria
- [ ] Trace logging (`RUST_LOG=aws_smithy_runtime=trace`) shows retry attempts
- [ ] Trace logging shows when errors are preserved vs cleared during rewind
- [ ] Documentation explains how to enable trace logging for debugging

## Technical Design

### Fix Location
- **File**: `rust-runtime/aws-smithy-runtime-api/src/client/interceptors/context.rs`
- **Method**: `rewind()`

### Proposed Change
```rust
// Before (buggy):
self.output_or_error = None;

// After (fixed):
if let Some(Ok(_)) = self.output_or_error {
    self.output_or_error = None;
}
```

### Error Types Covered
All `OrchestratorError` variants will be preserved:
1. `Interceptor` - errors from interceptors
2. `Operation` - service errors
3. `Timeout` - request timeout errors
4. `Connector` - HTTP connection/dispatch errors
5. `Response` - deserialization errors
6. `Other` - general errors

### Related Files
- `rust-runtime/aws-smithy-runtime-api/src/client/orchestrator.rs` - `OrchestratorError` enum
- `rust-runtime/aws-smithy-runtime/src/client/orchestrator.rs` - Retry loop logic

## Testing Requirements

### Unit Tests
- [ ] Test that `rewind()` preserves `Err` variants in `output_or_error`
- [ ] Test that `rewind()` clears `Ok` variants in `output_or_error`
- [ ] Test that `rewind()` handles `None` correctly

### Integration Tests
- [ ] Test timeout during retry returns error, not `Ok()`
- [ ] Test connector error during retry returns error, not `Ok()`
- [ ] Test successful retry after transient failure returns `Ok()` with correct response

## References

- **Fix Branch**: `fix-timeout-ok-bug`
- **Related Tickets**: 
  - P358713763 (Customer report)
  - P356524108 (ELB team confirmation - request never received)
  - V2042447957 (CMI team ticket)
