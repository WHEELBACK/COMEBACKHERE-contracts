# 🛡️ Maintainer Security & Pre-Merge Checklist

This checklist is mandatory for all repository maintainers prior to approving or merging any Pull Request into `master` or `main`. It prevents supply-chain attacks, Trojan horse commits, and obfuscated malware from compromising our ecosystem.

---

## 🤖 Tier 1: Automated Checks (Must be Green)

Before reviewing code manually, verify that GitHub Actions has executed and passed all status checks:
- [ ] **`Supply chain & anti-malware scan`** (via `.github/workflows/pr-malware-guard.yml`): Validates line length, known malware signatures, lifecycle hooks, and `.vscode` configs.
- [ ] **`Security policy gate`** (via `.github/workflows/security-gate.yml`): Verifies dependency vulnerabilities, SAST, and secrets scanning.
- [ ] **No bypassed checks**: Ensure no administrator override was used to bypass failing status checks.

---

## 🔍 Tier 2: Manual Code & Scope Review (60-Second Inspection)

Review the **Files changed** tab on GitHub:

### 1. Scope & Intent Match
- [ ] **Targeted Changes**: Does the PR change only what is stated in the linked issue description?
- [ ] **Scope Discrepancy**: If the PR claims to be a simple fix or doc update, why does it touch 50+ files or package configs? *Flag immediately if disproportionate.*

### 2. High-Risk Files Inspection
Examine the following files with extra scrutiny if modified:
- [ ] **Configuration Files**: `next.config.*`, `vite.config.*`, `postcss.config.*`, `tailwind.config.*`, `webpack.config.*`, `tsconfig.json`.
  - Look for unusual horizontal scrollbars.
  - Check that the file ends cleanly on the final export without hundreds of trailing spaces.
- [ ] **IDE & Environment Files**: `.vscode/tasks.json`, `.vscode/settings.json`, `.devcontainer/`.
  - Reject any PR that introduces `"runOn": "folderOpen"` or `"task.allowAutomaticTasks": "on"`.
- [ ] **Package Declarations**: `package.json`, `Cargo.toml`, `requirements.txt`.
  - Verify no unknown dependencies are being introduced.
  - Reject PRs adding unexpected `preinstall` or `postinstall` commands that download binaries, execute shell commands, or run arbitrary scripts.

### 3. Binary & Font Files
- [ ] Reject any PR introducing unusual font files (`.woff2`, `.eot`, `.lfk`, `.llf`) outside of vetted asset directories.
- [ ] Reject compiled JavaScript files in source PRs (e.g., `dist/setup.js`, `api.js`).

---

## 🔐 Tier 3: Contributor & Commit Hygiene

- [ ] **Fork Sanity**: Review contributor profile for brand-new accounts with no activity submitting large, complex diffs.
- [ ] **No Force-Pushed Re-merges**: Ensure commits do not contain nested, disguised merge commits that rewrite history.
- [ ] **GPG Signed Commits (Preferred)**: Verify commits are marked with GitHub's **Verified** badge where possible.

---

## 🚨 Incident Response: If Malware is Found

If a PR exhibits any suspicious payload:
1. **DO NOT MERGE.**
2. Close the PR immediately with an explanation citing security policy.
3. Block the submitting user from the GitHub organization/repository.
4. Report the account and PR URL to GitHub Support / Trust & Safety.
