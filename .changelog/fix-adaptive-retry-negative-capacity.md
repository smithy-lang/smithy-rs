---
applies_to:
- aws-sdk-rust
- client
authors:
- vcjana
references:
- smithy-rs#4632
breaking: false
new_feature: false
bug_fix: true
---

Fix adaptive retry rate limiter to never allow negative token bucket capacity. Previously, `acquire_permission_to_send_a_request` unconditionally deducted the request cost even when returning a delay, causing capacity to go negative. With multiple concurrent tasks, this produced cascading sleep times proportional to the number of tasks (e.g., task 50 sleeping 100s), leading to near-zero request rates that never recovered after a throttling event. Now, capacity is only deducted when a token is actually granted, and the orchestrator re-acquires after sleeping to account for concurrent state changes.
