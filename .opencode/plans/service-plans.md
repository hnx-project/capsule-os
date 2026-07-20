# CapsuleOS — 核心系统服务架构与实现蓝图

本规范定义了 **CapsuleOS (代号: Pangu)** 在 2.0 "Zero-POSIX Kernel" 时代下的全部核心用户态服务：职责边界、依赖关系、启动自举序列以及分阶段实现计划。

---

## 🧩 1. 核心服务清单

| # | 二进制名称 | PID | 职责说明 | 当前状态 |
| :--- | :--- | :--- | :--- | :--- |
| 1 | **`init`** | 1 | 首个用户态进程，从 BootFS VMO 加载并启动所有核心服务，监控生命周期 | ✅ 已有 loader 雏形，需重构 |
| 2 | **`devmgr`** | 2 | 硬件设备发现与初始化，平台总线枚举，FDT 解析，MMIO/中断管理 | ⏳ 存根 (19 行) |
| 3 | **`fileagent`** | 3 | 虚拟文件系统（VFS）守护进程，处理所有存储相关 IPC，管理 RamFS/BootFS | ⏳ 已有代码，需按 0-POSIX 重做 |
| 4 | **`procmgr`** | 4 | 管理进程树、fork/exec 逻辑、进程表维护 | ⏳ 已有代码，需按 0-POSIX 重做 |
| 5 | **`reincar`** | 5 | 监控关键服务状态，崩溃时自动重启 | ❌ 待实现 |
| 6 | **`netd`** | — | TCP/IP 协议栈服务（用户态网络栈） | ❌ 待实现 |
| 7 | **`secmgr`** | — | 权限策略配置、能力审计与安全管理 | ❌ 待实现 |
| 8 | **`logd`** | — | 集中式日志收集、缓存与持久化 | ❌ 待实现 |
| 9 | **`timed`** | — | 系统时间同步、定时器调度与 RTC 管理 | ❌ 待实现 |
| 10 | **`powermgr`** | — | 休眠、唤醒、频率调节与电量管理 | ❌ 待实现 |
| 11 | **`dispdrv`** | — | 图形输出底层驱动（framebuffer / GPU） | ❌ 待实现 |
| 12 | **`inputd`** | — | 键盘、鼠标、触摸屏事件收集与分发 | ❌ 待实现 |
| 13 | **`storaged`** | — | 磁盘分区识别、块设备管理与挂载协调 | ❌ 待实现 |
| 14 | **`crashd`** | — | 记录并存储 `.ohcrash` 崩溃转储信息 | ❌ 待实现 |

---

## ⛓️ 2. 服务依赖关系

```
              ┌─────── loader (PID 1) ────────┐
              │         (启动锚点)            │
              │  从 BootFS VMO 拉起所有服务    │
              └─────────┬────────────────────┘
                        │
           ┌────────────┼────────────┐
           ▼            ▼            ▼
       devmgr (2)  fileagent (3)  reincar (5)
       ┌────┴─┐         │            │
       ▼      ▼         ▼            ▼
   dispdrv  inputd   procmgr (4)  (监控全部)
   powermgr  netd       │
    storaged            ├── secmgr
                        ├── logd
                        ├── timed
                        ├── crashd
                        └── (普通应用)
```

### 关键依赖规则

| 服务 | 依赖 | 原因 |
| :--- | :--- | :--- |
| **fileagent** | 无（仅 VMO + Channel） | 纯内存操作，Bootstrap 阶段即可运行 |
| **devmgr** | 内核扩展（MMIO Map / DTB / IRQ） | 硬件访问需要内核提供用户态能力 |
| **procmgr** | fileagent | 需要 VFS 读取二进制文件实现 exec |
| **reincar** | init（wait4 通知） | 只需监听子进程退出事件 |
| **其余服务** | fileagent ± devmgr | 依赖 VFS 读取配置，硬件驱动依赖 devmgr |

---

## 🚀 3. 启动自举序列

```
Bootloader → 物理多段加载 (Kernel + BootFS VMO + DTB)
      │
      ▼
  Kernel init → 包装 BootFS VMO → 拉起 init (PID 1)
      │
      ▼
  init (PID 1)
      │  ├─ 创建 svc.devmgr 通道 ──→ devmgr (PID 2)
      │  ├─ 创建 svc.vfs    通道 ──→ fileagent (PID 3)
      │  ├─ 创建 svc.procmgr 通道 ──→ procmgr (PID 4)
      │  ├─ 创建 svc.reincar 通道 ──→ reincar (PID 5)
      │  └─ 注册自身 svc.init 通道
      │
      ▼
  Bootstrap 完成 ──→ reincar 监控 ──→ 其余服务通过 fileagent + procmgr 启动
```

所有 Bootstrap 层服务（init / devmgr / fileagent / procmgr / reincar）必须遵循 0-POSIX / 0-VFS / 0-FD 开发标准，仅依赖 `libcapsule` 的无状态能力原语。

---

## 📐 4. 自举服务开发标准

参见 [posix-plan.md](posix-plan.md) 第 4-5 节。核心约束：

1. **严禁链接 `libc` 的有状态 I/O** — 不调用 `open`/`read`/`write`/`printf`
2. **强依赖 `libcapsule` 原语** — `vmo_read` / `channel_create` / `channel_write` / `channel_read`
3. **0-VFS 调试打印** — 使用 `libcapsule::kprintln!` 或 `sys_write_debug`

---

## 📦 5. 服务间通信协议风格

| 服务 | 注册名称 | 协议风格 | 备注 |
| :--- | :--- | :--- | :--- |
| **init** | `svc.init` | 简单请求/响应 | 查询服务列表、触发重启 |
| **devmgr** | `svc.devmgr` | 请求/响应 | 查询设备树、申请 MMIO 区域 |
| **fileagent** | `svc.vfs` | 会话式（多 session） | open → session_chan → read/write/close |
| **procmgr** | `svc.procmgr` | 请求/响应 | 创建进程、查询进程表 |
| **reincar** | `svc.reincar` | 单向通知 | 监听崩溃事件 |
| **其余服务** | `svc.<name>` | 视具体设计 | — |

---

## 🗺️ 6. 分阶段实现计划

### Phase 1 — Bootstrap 核心重构（当前阶段）

自举层服务全部按 0-POSIX 标准重做，建立可复用的纯 `libcapsule` 模式。

| 顺序 | 服务 | 目标 | 前置条件 |
| :--- | :--- | :--- | :--- |
| **①** | **fileagent** | IPC 会话式 VFS 服务，管理 RamFS + BootFS VMO，注册 `svc.vfs` | 无（仅已有 syscall） |
| **②** | **devmgr** | IPC 骨架 + 平台信息查询，注册 `svc.devmgr`；MMIO/DTB 功能待内核扩展 | fileagent（参考模式） |
| **③** | **procmgr** | IPC 进程管理服务，注册 `svc.procmgr` | fileagent（参考模式） |
| **④** | **﹝可选﹞reincar** | 进程监控与重启服务 | init（wait4 支持） |
| **⑤** | **init** | 重构为完整服务管理器（重命名 loader → init，支持动态启动/重启） | ① ② ③ 完成后按新模式重写 |

### Phase 2 — 内核扩展（与 Phase 1 并行 or 随后）

| 特性 | 说明 | 服务收益 |
| :--- | :--- | :--- |
| MMIO 映射 syscall | 物理地址 → VMAR 映射 | devmgr 可操作硬件寄存器 |
| DTB 暴露给 userspace | FDT 数据通过 VMO 读取 | devmgr 可枚举设备树 |
| IRQ 绑定 syscall | IRQ → Channel 绑定 | devmgr 可处理中断 |
| FDT 解析库移植 | 从 kernel 移植到 userspace `libcapsule` | devmgr 可自行解析 DTB |

### Phase 3 — 增值服务

依赖 VFS + 进程管理的上层服务：

- secmgr / logd / timed / crashd

### Phase 4 — 硬件驱动服务

依赖内核扩展 + devmgr：

- netd / powermgr / dispdrv / inputd / storaged

---

*本文档由 CapsuleOS 工程 AI 辅助架构师 opencode 与 HNX-Project 管理组共同制定。*
