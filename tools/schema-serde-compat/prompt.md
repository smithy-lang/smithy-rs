Continue the schemaSerde compatibility audit from C10 in this repository.

Read AGENTS.md and tools/schema-serde-compat/{findings.json,divergence.md,README.txt}. Preserve all unrelated working-tree changes.

Previous decisions:
- C06: accepted divergence; preserve current behavior.
- C07: Document @httpPayload null preservation fixed and committed as c11f75e97. The separate Document-field discrepancy remains unresolved.
- C08: accepted float/double overflow to signed infinity.
- C09: accepted exact integer precision instead of legacy precision loss.
- C08/C09 are recorded in divergence.md, but that documentation edit was not committed.
- Fresh C08/C09 evidence: tools/schema-serde-compat/.runs/20261014T204419.010668Z/.

Start with C10, “Document nesting limits differ.”
1. Read the finding and relevant legacy/schema code paths.
2. Freshly generate legacy and schemaSerde servers from identical models, with http-1x: true and schemaSerde false/true.
3. Reproduce the nesting-limit difference, testing depths around both boundaries.
4. Compare status, headers, exact response bodies, and handler-observed values. Retain reproducible evidence.
5. Check relevant Smithy specifications and distinguish explicit requirements from interpretation.
6. Report the confirmed behavior and recommend whether to fix or accept the divergence.

Do not change implementation behavior until the legacy baseline is established and we have discussed the result. Do not commit, push, or proceed to C11. Do not delete directories or use destructive Git operations.
