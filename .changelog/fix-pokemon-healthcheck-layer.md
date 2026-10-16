---
applies_to: ["server"]
authors: ["sachinsharma3191"]
references: ["smithy-rs#4631", "smithy-rs#3607", "smithy-rs#3606"]
breaking: false
new_feature: false
bug_fix: true
---
Move `AlbHealthCheckLayer` in the pokemon-service example from B-position (inside `PokemonServiceConfig`) to A-position (wrapping the router). The layer was previously registered on `/ping`, which is already owned by the modeled `CheckHealth` operation, so the health check handler was never reached. It now listens on `/health` and wraps the final service so requests are answered before routing.
