# Process Manager (procmgr) Implementation Plan

## 诊断摘要

我们追踪了两个相互独立的 Corruption：

1. **Loader 的 L3 页 (0x4022b000)** 在 osh 的第一个 `map_page_under_l0` 调用中被清零
2. **Osh 自己的 L3 页 (0x40264000)** 在 CREATION 和 LAUNCH-ADD 之间被破坏 (entries 1-3 变 0/脏数据)

所有 WATCHDOG 都证实：`alloc_page` 从未返回已分配的页，`zero_page`/`write_pte` 也从未直接写入违规地址。但 watchdog 检查到的正是 corruption 发生的瞬时：

```
LAUNCH-WATCH | L3@4022b000[0]=0x0060000040227743 before map target_va=0xc0200000
LAUNCH-WATCH | L3@4022b000[0]=0x0000000000000000 after map target_va=0xc0200000
NEXT_FREE_PAGE: 0x40245000 → 0x40247000
```

两次 alloc 分别是 L1(0x40245000) 和 L2(0x40246000)，两者都不在 0x4022b000 附近。

**已确定的事实**：
- L1 页全新分配，不会与已有页表冲突
- 没有代码显式写 0x4022b000
- **但物理内存被零了** — 这只能在 QEMU TCG 上通过 KVA 写产生
- KVA = pa + 0xFFFF_8000_0000_0000，不会两个不同 PA 映射到同个 KVA

**可能的根因 (仍需确认)**：
- 写时复制 (COW) 缺失 — 页表复制时 L1[0..2] 从父进程照搬，导致子进程**共享父进程的 L2 页**。如果 devmgr/fileagent 的创建过程中 shatter 操作修改了共享 L2 页，可能通过某种缓存一致性路径波及无关页
- 或者：问题是 COW/引用计数缺失的级联效应，不是单一 bug

## 方案：实现 procmgr

### 架构

```
Bootloader → HNX Kernel → devmgr → fileagent → **loader → procmgr → osh (shell)**
                                                ↓
                                          其他 EL0 服务
```

procmgr 是一个运行在 EL0 的**系统服务进程**，拥有内核信任的 IPC 通道。它接管：

1. **进程表** — 全局统一管理 (PID → 元数据)
2. **父子关系** — fork/spawn 时记录 parent_pid
3. **Zombie 回收** — 进程退出时标记 Zombie，wait 时安全回收
4. **页表生命周期** — 回收时通知内核释放 L0→L1→L2→L3 的整棵树

### 内核侧改动

#### 1. 新增系统调用 `SYS_PROC_MGMT`

```
cmd=0: PROC_MGMT_CREATE(parent_pid, name_ptr, l0_pa) → pid
cmd=1: PROC_MGMT_EXIT(pid) → status
cmd=2: PROC_MGMT_WAIT(pid) → child_exit_status
cmd=3: PROC_MGMT_LIST(buf, max) → count
cmd=4: PROC_MGMT_RELEASE_PT(l0_pa) → status  // 释放整棵页表树
```

#### 2. PID 分配迁移到 procmgr

当前内核 `PROCESS_ID_COUNTER` 自增，改为由 procmgr 通过 `PROC_MGMT_CREATE` 统一分配。内核调用流程：

```rust
// 当前：内核内部自增
let pid = PROCESS_ID_COUNTER.fetch_add(1, ...);

// 改为：向 procmgr 申请
let pid = procmgr_create_process(parent_pid, name, l0_pa);
```

#### 3. 页表树安全释放

新增函数 `free_page_table_tree(l0_pa)`，递归遍历释放 L0→L1→L2→L3 的所有页表页。在 procmgr 确认回收后，由内核执行。

```rust
fn free_page_table_tree(l0_pa: usize) {
    for l0_idx in 0..512 {
        let l0e = read_pte(l0_pa, l0_idx);
        if l0e & 1 == 0 || l0e & 2 == 0 { continue; }
        let l1_pa = extract_pa(l0e);
        for l1_idx in 0..512 {
            let l1e = read_pte(l1_pa, l1_idx);
            if l1e & 1 == 0 { continue; }
            if l1e & 2 == 0 { continue; }  // block → skip
            let l2_pa = extract_pa(l1e);
            for l2_idx in 0..512 {
                let l2e = read_pte(l2_pa, l2_idx);
                if l2e & 1 == 0 { continue; }
                if l2e & 2 == 0 { continue; }  // block → skip
                let l3_pa = extract_pa(l2e);
                free_page(l3_pa);  // L3 页表页
            }
            free_page(l2_pa);  // L2 页表页
        }
        free_page(l1_pa);  // L1 页表页
    }
    free_page(l0_pa);  // L0 页表页
}
```

#### 4. UART 映射不依赖 L1 复制

当前代码复制父进程 L1[0..2] 到子进程，导致共享。改为：

- 子进程 L1 页的 entries 0, 1, 2 从 boot 页表复制 block entries，而不是从父进程复制
- UART 按需映射，不依赖父进程已有的 shatter 状态

#### 5. 进程退出信号

子进程退出时：
1. 内核调用 `PROC_MGMT_EXIT(pid)` → procmgr 将进程标记 Zombie
2. 内核调度父进程，通知它调用 `PROC_MGMT_WAIT(pid)`
3. procmgr 确认后，内核执行 `PROC_MGMT_RELEASE_PT(l0_pa)`

### 用户态 procmgr 实现

`userspace/services/procmgr/` 下新项目：

```rust
// 核心数据结构
static PROC_TABLE: [Option<ProcEntry>; 256] = [None; 256];

struct ProcEntry {
    pid: u64,
    ppid: u64,
    state: ProcState,  // Running | Zombie
    name: [u8; 32],
    l0_pa: u64,
}
```

### 启动时序变更

1. loader 启动
2. loader 启动 devmgr + fileagent
3. loader 启动 procmgr（传递特权能力）
4. procmgr 初始化进程表
5. loader 通过 procmgr 启动 osh
6. osh 通过 procmgr 启动 ls/ps/cat 等

### 分步实施

| 步骤 | 内容 | 依赖 |
|------|------|------|
| 1 | 内核新增 `SYS_PROC_MGMT` syscall stub | 无 |
| 2 | 内核实现 `free_page_table_tree` | 步骤 1 |
| 3 | 修复 L1 复制逻辑 (非共享页表) | 步骤 1 |
| 4 | 创建 `userspace/services/procmgr` 项目 | 步骤 1 |
| 5 | procmgr 实现进程表、Zombie 回收 | 步骤 2, 4 |
| 6 | 修改 loader 启动时序，加入 procmgr | 步骤 5 |
| 7 | 修改内核退出流程，通知 procmgr | 步骤 5 |
| 8 | 测试：启动 osh → ls → 退出 → 再次启动 osh | 步骤 6, 7 |
