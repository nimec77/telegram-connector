# Development Workflow

**Reference:** [tasklist.md](tasklist.md) | [vision.md](vision.md) | [conventions.md](conventions.md) | [memory.md](memory.md)

---

## Iteration Cycle

```
┌──────────────────────────────────────────────────────────────────────┐
│  1. PROPOSE  →  2. AGREE  →  3. IMPLEMENT  →  4. VERIFY              │
│       ↑                                            │                 │
│       │                 5. UPDATE PROGRESS  →  6. UPDATE MEMORY      │
│       └────────────────────────────────────────────────────────────┘ │
│                         Next Iteration / Phase                       │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Step 1: PROPOSE

Before writing any code:

1. State current phase and task from `tasklist.md`
2. Describe the proposed solution briefly
3. Show code snippets for key parts:
   - Tests (TDD: tests first)
   - Public API signatures
   - Core implementation approach
4. Ask: **"Agree with this approach?"**

**Example:**
```
Phase 5, Task: Implement ChannelId type

Proposed solution:
- Newtype wrapper around i64
- Derive: Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize
- Implement Display for formatting

Test:
​```rust
#[test]
fn channel_id_display() {
    let id = ChannelId(123456);
    assert_eq!(format!("{}", id), "123456");
}
​```

Implementation:
​```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(pub i64);
​```

Agree with this approach?
```

---

## Step 2: AGREE

Wait for user confirmation before proceeding:

| Response | Action |
|----------|--------|
| "Yes" / "Agree" / "OK" | Proceed to Step 3 |
| "No" / Feedback | Revise proposal, return to Step 1 |
| Questions | Answer, then confirm agreement |

**Do NOT implement without explicit agreement.**

---

## Step 3: IMPLEMENT

After agreement:

1. Write tests first (TDD)
2. Implement minimal code to pass tests
3. **Run `cargo fmt --all`** to format code
4. Run `cargo test` to verify
5. Run `cargo clippy -- -D warnings` to check quality
6. Show implementation result

**IMPORTANT:** Always run `cargo fmt --all` after writing any code to ensure consistent formatting.

**Output format:**
```
Implemented: [brief description]

Files changed:
- src/telegram/types.rs (added ChannelId)

Tests: ✅ All pass (3 new tests)
Clippy: ✅ No warnings
Fmt: ✅ Formatted

Ready for verification.
```

---

## Step 4: VERIFY

Wait for user to confirm:

| Response | Action |
|----------|--------|
| "Confirmed" / "Good" | Proceed to Step 5 |
| "Issue: ..." | Fix issue, re-verify |
| "Revert" | Undo changes, return to Step 1 |

---

## Step 5: UPDATE PROGRESS

After verification:

1. Update `doc/tasklist.md`:
   - Mark completed tasks with `[x]`
   - Update phase status in Progress Report table
   - Update test counts
2. Update `CLAUDE.md`:
   - Update current status line
   - Update test counts if changed significantly
   - Update MCP tools table (if applicable)
   - Update Development Progress table
3. Show updated progress
4. Ask: **"Proceed to next task?"**

**Progress update format:**
```
Updated tasklist.md:
- [x] Write tests for ID types
- [x] Implement ChannelId

Updated CLAUDE.md:
- Test count: 129 → 132

Phase 5 progress: 2/4 tasks complete

Proceed to next task?
```

---

## Step 6: UPDATE MEMORY

After completing a phase or significant iteration:

1. Update `doc/memory.md` with:
   - **Progress made** - what was completed
   - **Patterns applied** - design decisions, architectural choices
   - **Lessons learned** - gotchas, edge cases discovered
   - **Code patterns to reuse** - snippets for future phases
2. Verify all three documentation files are updated:
   - `doc/tasklist.md` - task checkboxes and progress table
   - `doc/memory.md` - detailed notes and patterns
   - `CLAUDE.md` - current status and test counts
3. Ready for next phase

**IMPORTANT:** Use LOCAL file `doc/memory.md`, NOT global Claude memory.

**When to update documentation:**
- ✅ After completing each phase
- ✅ After completing significant iterations (multiple tasks)
- ✅ When discovering important patterns or gotchas
- ✅ When test counts change significantly
- ❌ Not after every single trivial task

**Memory update format:**
```markdown
## Phase N: [Name] (Complete)

### What Was Implemented
- Component 1 with key details
- Component 2 with key details

### Key Decisions & Rationale
1. Decision: Why we chose this approach

### Gotchas & Edge Cases
1. Issue: How we solved it

### Patterns to Reuse
```rust
// Code pattern with explanation
```
```

---

## Rules

### Must Follow

1. **Strict order** — Follow `tasklist.md` phases sequentially
2. **TDD always** — Tests before implementation
3. **No skipping** — Complete all tasks in phase before next
4. **Explicit agreement** — Wait for "yes" before implementing
5. **Explicit verification** — Wait for confirmation before updating progress

### Must Not

1. **No autonomous progression** — Never proceed without user confirmation
2. **No bulk implementation** — One task at a time
3. **No assumption** — If unclear, ask first
4. **No skipping tests** — Every feature needs tests

---

## Phase Transition

Before starting a new phase:

1. Verify all tasks in current phase are `[x]`
2. Verify phase status is ✅ in Progress Report
3. State: **"Phase N complete. Start Phase N+1?"**
4. Wait for confirmation

---

## Quick Commands

User can say:

| Command | Meaning |
|---------|---------|
| "Continue" | Proceed with current plan |
| "Stop" | Pause work |
| "Show progress" | Display current tasklist status |
| "Skip to Phase N" | Jump to phase (requires confirmation) |
| "Revert" | Undo last implementation |

---

## Session Start

At the beginning of each session:

1. Read `tasklist.md` for current state
2. Report: current phase, completed tasks, next task
3. Ask: **"Continue from [task]?"**
