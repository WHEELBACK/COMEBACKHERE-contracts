I need to stop here and flag a problem with this task rather than produce file content.

**The step you've given me is a verification/escalation step, not an implementation step.** It explicitly says:

> Verify whether the `contracts/escrow-vault` and `contracts/dispute-resolver` directories exist... If they do not exist, **escalate before creating new crates**. This step is a prerequisite for all four issues... and must be completed before any code changes.

I cannot complete this step by returning file content, because:

1. **I have no repository access.** I can only see the single file you supplied (`contracts/escrow-vault/src/lib.rs`), and its "current content" is empty. I cannot list directories or confirm whether `contracts/escrow-vault` or `contracts/dispute-resolver` exist elsewhere in the repo.

2. **The step's own instruction is to escalate, not to write code.** Producing a `lib.rs` now would mean *creating a new crate* — exactly what the step forbids without escalation. Doing so would also violate the instruction to "return the COMPLETE new file content" only for a real change, since there is no existing content to preserve and no confirmed crate to modify.

3. **All four issues (#906, #907, #912, #915) depend on this prerequisite.** Writing an escrow-vault implementation from scratch would be inventing an API surface, storage layout, and error set that may conflict with the real crate (if it exists) or with maintainer intent (if it doesn't). That is not a safe assumption to make silently.

**What I need from you to proceed:**

- Confirmation of whether `contracts/escrow-vault/` and `contracts/dispute-resolver/` exist in the repository.
- If they exist: the actual current contents of each `src/lib.rs` (and any entrypoint/module files), so I can implement #906/#907/#912/#915 against the real code.
- If they do not exist: explicit approval to create the crates, plus the intended public interface (functions, storage keys, error enum, role model) so the new code matches project conventions.

Once I have either the existing files or approval plus a spec, I can implement the role-based authorization, storage-deserialization streamlining, and zero-balance handling with tests, and return complete file contents as requested.