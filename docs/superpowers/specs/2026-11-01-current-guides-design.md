# Reconcile current project guides

## Intent and scope

Bring contributor/onboarding documentation up to the pushed 0.24.1 state. Salvage
accurate improvements from `docs/current-project-state` without reintroducing its
obsolete pending Task 8–10 claims. All ten original tasks are accepted; optional
media M2/M3 still require a target-client decision. Historical validation reports
retain their original versions, measurements and review boundaries.

## Approaches

- Merge the old documentation commit wholesale: would restore obsolete roadmap
  claims and omit later recovery/media guarantees.
- **Implementer's choice:** port the useful configuration/onboarding corrections
  onto current master, retain current contracts, and remove already-merged Codex
  workflow plans/specs under the repository's archive-on-merge rule.
- Only update memory: fixes the version but leaves misleading paths and completed
  workflow artifacts in the active documentation.

## Deliverable and constraints

Update README, Codex guide, configuration comments, agent guidance, current memory
and minor runtime/contract wording where verified. Use source inspection, local
CLI help and official OpenAI documentation for factual claims. Link to existing
acceptance evidence. Change no Rust code, dependencies, active TOML values, package
version, historical reports, or optional media behavior. Preserve the old branch.

Run link/TOML/scope checks and `just check-docs` offline on macOS; obtain an independent
whole-diff review. A clean feature branch in the current checkout is sufficient;
no new worktree or Linux environment is needed. Execute inline under AGENTS.md's
no-routine-checkpoint policy; no separate design approval is required for this
authorized documentation task.
