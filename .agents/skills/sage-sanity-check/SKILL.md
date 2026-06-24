---
name: sage-sanity-check
description: >
  [Auto-run] MUST invoke before reporting code changes as complete. Trigger:
  any turn where you created or modified .rs files in the sage codebase. No
  exceptions — run even if tests pass and formatting is clean.
---

# Sage Sanity Check

Spawn a subagent to check for SAGE-specific anti-patterns. Pass it:

1. A description of what code was generated or modified (files, purpose, scope of change).
2. The instructions file at `references/patterns.md` found in this skill's directory.

Example invocation:

```
Agent({
  description: "Sage sanity check",
  prompt: `Review the following code changes for SAGE-specific anti-patterns.

Changed files: <list the modified/new files>
Purpose: <brief description of what was done>

Follow the instructions in .agents/skills/sage-sanity-check/references/patterns.md exactly. Read that file first, then check each pattern against the changed code. Report findings in the format specified.`
})
```

Do NOT attempt the checks yourself — delegate to the subagent so findings come from a fresh perspective without context bias from the generation pass.
