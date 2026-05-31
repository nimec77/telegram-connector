# Development Workflow

**Reference:** [tasklist.md](tasklist.md) | [vision.md](vision.md) | [conventions.md](conventions.md) | [memory.md](memory.md)

---

This repo is set up for **in-house team development** using the **superpowers**
skill-driven flow. There is no manual PROPOSE → AGREE approval gate, and git
operations (branch, commit, PR) are expected as part of normal work.

The defaults that follow are the recommended path, not a permission system —
adapt them to the size of the change.

---

## Iteration Cycle

```
brainstorming → writing-plans → test-driven-development → implement
      ↑                                                       │
      │                  requesting-code-review  ←────────────┘
      │                            │
      └──── finishing-a-development-branch (merge / PR) ──────┘
                         Next change / phase
```

Each arrow is a superpowers skill — invoke it via the `Skill` tool when you
reach that step.

---

## Step 1: Brainstorm

Use the **brainstorming** skill before any creative work (new feature,
behaviour change, non-trivial refactor). Explore intent and requirements before
touching code. Skip it only for mechanical, well-scoped edits.

## Step 2: Write the plan

Use the **writing-plans** skill to turn the agreed direction into a plan.

- Design docs land in `docs/superpowers/specs/` (one dated file per change).
- Implementation plans land in `docs/superpowers/plans/` (one dated file per change).
- For multi-phase work, also reflect phases/tasks in `docs/tasklist.md`.

## Step 3: Implement with TDD

Use the **test-driven-development** skill.

1. Write the failing test first — no production code without a preceding test.
2. Implement the minimal code to pass the test.
3. Run `cargo fmt --all` after every code change (not just `--check`).
4. Run `cargo test` and `cargo clippy -- -D warnings`.

**IMPORTANT:** Always run `cargo fmt --all` after writing any code to keep
formatting consistent.

## Step 4: Review before merge

Use the **requesting-code-review** skill before merging a branch. Address
feedback with the **receiving-code-review** skill (verify, don't rubber-stamp).

## Step 5: Finish the branch

Use the **finishing-a-development-branch** skill to choose how to integrate the
work (merge, PR, or cleanup). Commits and PRs are fully allowed.

---

## Pre-merge Gate

All must pass before merging:

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

(Config tests mutate env vars — run them serially: `cargo test config -- --test-threads=1`.)

---

## Git Conventions

- Branch, commit, and open PRs freely. Use feature branches (and worktrees, via
  the **using-git-worktrees** skill) to isolate work.
- History follows conventional-commit style: `feat:`, `fix:`, `chore:`,
  `docs:`, `refactor:`, `test:`.
- Releases are tagged with `chore: release vX.Y.Z` after the pre-merge gate
  passes.

---

## Keeping Docs in Sync

After completing a phase or a significant change, update:

1. `docs/tasklist.md` — mark tasks `[x]`, update phase status and test counts.
2. `docs/memory.md` — progress made, patterns applied, lessons learned, and
   reusable code patterns. This is the **local** project journal; keep project
   knowledge here, not in global Claude memory.
3. `CLAUDE.md` — only when architecture, tooling, dependency versions, or the
   workflow itself changes.

**When to update:**

- ✅ After completing a phase or significant iteration
- ✅ When discovering important patterns or gotchas
- ✅ When dependency versions or architecture change
- ❌ Not after every trivial task

**Memory entry format:**

```markdown
## Phase N: [Name] (Complete)

### What Was Implemented
- Component with key details

### Key Decisions & Rationale
1. Decision: why we chose this approach

### Gotchas & Edge Cases
1. Issue: how we solved it

### Patterns to Reuse
​```rust
// Code pattern with explanation
​```
```

---

## Session Start

At the beginning of each session, skim `docs/tasklist.md` for the current phase
and open tasks before picking up work.
