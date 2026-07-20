---
name: capsule-contributing
description: Use ONLY when preparing, reviewing, or submitting Pull Requests (PRs), conventional commit messages, or syncing branch branches. Trigger on CONTRIBUTING.md.
---

# CapsuleOS Contribution & Workflow Skill

Use this skill when staging commits, pushing features, or reviewing incoming community modifications.

## 🤝 Subtree Monorepo Simplified Workflow
Thanks to Git Subtrees, contributors only need to fork a single repository (`capsule-os`) without managing Submodules.

## 🔄 PR Execution Loop
1.  Fork `capsule-os` and checkout to a local feature branch off **`develop`**.
2.  Implement and compile changes with `xtask code build --arch aarch64`.
3.  Ensure non-regression with `testall` running inside QEMU.
4.  Commit with Conventional Commit messages (`feat`, `fix`, `docs`, `refactor`, `style`, `perf`, `test`, `chore`).
5.  Push to branch and open a Merge Request targeting the upstream **`develop`** branch.
