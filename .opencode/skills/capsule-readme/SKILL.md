---
name: capsule-readme
description: Use ONLY when reading, explaining, or restructuring the CapsuleOS main portal, quickstart procedures, and index. Trigger on README.md.
---

# CapsuleOS Portal Entry Skill

Use this skill to guide users or subagents through setting up CapsuleOS, running simple builds, and navigating the overall documentation index.

## 🔑 Operational Guidelines
*   **Core Entrance**: Keep `README.md` strictly lightweight and high-cohesion. Focus only on "What is CapsuleOS", "Quickstart", "Project Status", and "License".
*   **No Spoilers**: Do not bloat this file with detailed assembly architectures, capability lock invariants, or coding style details. Refer readers to `DEVELOPMENT.md` and `ARCHITECTURE.md`.
*   **Commands**:
    *   Setup: `./install_xtask`
    *   Build: `xtask code build --arch aarch64`
    *   Run: `xtask code run --arch aarch64`
