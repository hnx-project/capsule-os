# 🤝 Contributing to CapsuleOS

Thank you for your interest in contributing to **CapsuleOS (Current Version: Pangu 1.0.0.beta)**! We welcome code modifications, documentation updates, and bug fixes. 

---

## 🌌 The Power of Git Subtrees (No-Pain Monorepo)

Unlike traditional microkernel projects that rely on complex Git Submodules, CapsuleOS utilizes a **unified Git Subtree Monorepo** architecture. This provides an incredibly simple, friction-free experience for external developers:

*   **No Multi-Repo Forking**: You do **not** need to separately fork and link `hnx-core`, `capsule-bootloader`, or `ohlink-cc`. 
*   **Single-Fork Workflow**: You only need to fork a single repository: `capsule-os`.
*   **Unified Tree Modifying**: You can edit the kernel, bootloader, userspace libraries, and applications simultaneously within a single commit and a single PR.

---

## 🚀 Step-by-Step: How to Start a Pull Request

Follow this clear sequence to build, verify, and submit your contribution:

### Step 1: Fork and Clone the Main Repository
1.  Fork `capsule-os` to your personal account on GitCode or GitHub.
2.  Clone your personal fork locally:
    ```bash
    git clone https://gitcode.com/YOUR_USERNAME/capsule-os.git
    cd capsule-os
    ```

### Step 2: Create a Local Branch from `develop`
All active development, integration, and feature testing occur on the `develop` branch.
1.  Checkout and pull the latest changes from `develop`:
    ```bash
    git checkout develop
    git pull origin develop
    ```
2.  Create a clean, descriptive branch for your feature or bug fix:
    ```bash
    git checkout -b feat/your-awesome-feature
    ```

### Step 3: Implement Your Changes
Make modifications across any directory in the repository (e.g., `kernel/src/`, `bootloader/src/`, `libraries/libc/`). Ensure you adhere to the **8 Core Development Standards** documented in **[DEVELOPMENT.md](./DEVELOPMENT.md)**.

### Step 4: Run Environment Checks and Build
1.  Bootstrap the companion toolchain:
    ```bash
    ./install_xtask
    ```
2.  Verify your local host environment is aligned:
    ```bash
    xtask code check-env
    ```
3.  Compile the entire operating system, libraries, and applications for AArch64 (ensuring zero compiler warnings):
    ```bash
    xtask code build --arch aarch64
    ```

### Step 5: Emulate and Execute Tests
1.  Launch CapsuleOS inside the QEMU emulator:
    ```bash
    xtask code run --arch aarch64
    ```
2.  In the emulator, execute the `testall` suite to verify that all 12 system-level and filesystem integrity checks pass perfectly:
    ```text
    ===== All Test Suite =====
    [PASS] connect
    [PASS] create_file
    ...
    12/12 passed
    ```

### Step 6: Commit Your Changes (Conventional Commits)
Staging and committing use standard Git commands. Your commit messages must follow the [Conventional Commits](https://www.conventionalcommits.org/) format to maintain automated changelogs:

```bash
git add .
git commit -m "feat(kernel): add secure capability boundary checks for VMAR mapping"
```

*Refer to **[CONTRIBUTING.md commit type rules](#-allowed-commit-types)** below for formatting standards.*

### Step 7: Push and Open a Pull Request (PR)
1.  Push your branch to your personal remote fork:
    ```bash
    git push -u origin feat/your-awesome-feature
    ```
2.  Go to the GitCode or GitHub web interface of your fork, select your branch, and click **New Pull Request** or **New Merge Request**.
3.  **Target Branch Selection**: Ensure your PR targets the official upstream **`develop`** branch of `capsule-os`.

---

## 📜 Allowed Commit Types

Your commit headers must start with one of the following conventional types:

*   `feat`: A new feature (e.g., adding an API, system call, or driver).
*   `fix`: A bug fix (e.g., resolving a lock deadlock, memory leak, or crash).
*   `docs`: Documentation changes only.
*   `refactor`: Code changes that neither fix a bug nor add a feature.
*   `style`: Formatting, missing semi-colons, etc.; no production code change.
*   `perf`: A code change that improves performance.
*   `test`: Adding missing tests or correcting existing tests.
*   `chore`: Updating build scripts, dependencies, or tool configurations.

---

## 🛡️ Integration and Merge Criteria

Before a pull request can be merged into `develop`, it must pass our static and dynamic verification standards:

1.  **Strict Isolation**: Low-level, CPU-dependent assembly and register manipulations must be kept isolated inside `kernel/src/arch/aarch64`.
2.  **No Diagnostic Output Left**: Temporary debug lines or print dumps used during troubleshooting must be entirely cleaned up.
3.  **Green Tests**: The PR must successfully boot and pass all 12 tests inside the `testall` test runner.

---
*Designed and engineered with passion by **TinchyChin** and the **HNX-Project** community.*
