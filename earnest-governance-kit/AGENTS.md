# Earnest Governance & BCPE Rules

When working in this repository, follow the **Contract-First + Shadow Worktree** workflow:

1. **Consult Experience First**: Before coding on niche or strict stacks, call `kb_list_products` / `kb_search`.
2. **Define & Seal Contracts**:
   - Call `gov_add_invariant` for each verifiable requirement (`cargo_test` or `static`).
   - Call `gov_seal_architecture` to lock `.ide/invariants.json` (SHA-256) and switch to `constrained` mode.
3. **Code in Shadow Worktree**:
   - Stage changes with `gov_write_code(..., stage_only: true)`.
   - Run fast checks with `gov_shadow_check`.
   - Validate all sealed probes + `project-integrity-gate` and merge to the real workspace with `gov_shadow_validate_and_merge`.
4. **Capitalize Discoveries**: When you solve a non-obvious bug or pitfall, record it via `kb_propose`.
