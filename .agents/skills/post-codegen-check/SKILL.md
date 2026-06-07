---
name: post-codegen-check
description: >
  [Auto-run] MUST invoke before reporting code changes as complete. Trigger:
  any turn where you created or modified .rs files. No exceptions — run even
  if tests pass and formatting is clean.
---

# Post-Codegen Check

Spawn a subagent to perform the quality review. Pass it:

1. A description of what code was generated or modified (files, purpose, scope of change).
2. The instructions file at `references/review-instructions.md` found in this skill's directory.

Example invocation:

```
Agent({
  description: "Post-codegen quality check",
  prompt: `Review the following code changes for quality issues.

Changed files: <list the modified/new files>
Purpose: <brief description of what was done>

Follow the instructions in .agents/skills/post-codegen-check/references/review-instructions.md exactly. Read that file first, then execute each check. Report findings in the format specified.`
})
```

Do NOT attempt the checks yourself — delegate to the subagent so findings come from a fresh perspective without context bias from the generation pass.
