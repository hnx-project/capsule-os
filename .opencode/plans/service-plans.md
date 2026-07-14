# CapsuleOS 用户态服务实现规划

## 命名规则

`功能 + mgr/d/agent`

- `devmgr` — 设备管理器 (已有)
- `fileagent` — 文件系统服务 (已有)
- `loader` — 系统初始化 (代替 `init`，已有)
- `procmgr` — 进程管理器 (已规划)
- `osh` — 交互式 Shell (已有)
- `ls`/`cat`/`mkdir`/`touch`/`rm`/`rmdir`/`ps`/`kill` — 基础工具 (已有)

---

## Phase 1：基础服务层

### 1.1 `logd` — 日志服务

**职责**：统一接收内核和所有用户态服务的日志消息，按级别过滤，输出到 UART 和/或环形缓冲区。

**架构**：
```
┌─────────┐  IPC    ┌──────────┐
│ kernel  │ ──────→ │          │
├─────────┤         │  logd    │
│ devmgr  │ ──────→ │          │
├─────────┤         │ (EL0)    │
│ procmgr │ ──────→ │          │
└─────────┘         └────┬─────┘
                         │
                    ┌────┴─────┐
                    │  UART    │
                    └──────────┘
```

**实现步骤**：

| 步 | 内容 | 内核改动 |
|----|------|---------|
| 1 | 创建 `userspace/services/logd` 项目 (no_std) | 无 |
| 2 | 实现 IPC 服务端：注册到 `devmgr`，监听连接 | 无 |
| 3 | 定义日志协议：`LogMsg { level, source, msg }` | 需新增 syscall 或共享内存通道 |
| 4 | 内核 `log_info!` 宏改为通过 IPC 发往 `logd` (fallback 到 UART) | 修改日志宏 |
| 5 | 实现环形缓冲区 + UART 输出 | 无 |
| 6 | 加入启动时序：loader → devmgr → **logd** → procmgr → osh | 修改 loader |

**关键设计决定**：
- `logd` 启动时打开 UART 设备 (通过 `devmgr`)
- 采用**共享内存 + 单向通道**避免日志写入阻塞发送方
- 日志级别: Error / Warn / Info / Debug，可在运行时动态调整

---

## Phase 2：系统兼容层

### 2.1 `posixd` — POSIX 兼容服务

**职责**：将 POSIX 系统调用 (`open`/`read`/`write`/`fork`/`exec` 等) 转换为 CapsuleOS 的原生 IPC 调用。使得标准 C 库 (如 `hnxlibc`) 和用户程序无需修改即可运行。

**架构**：
```
┌───────────┐  POSIX API    ┌──────────┐
│ 程序       │ ────────────→ │          │
│ (libc)    │               │  posixd  │
└───────────┘               │  (EL0)   │
                            └────┬─────┘
                    ┌────────────┼────────────┐
                    ↓            ↓            ↓
               ┌────────┐  ┌────────┐  ┌──────────┐
               │procmgr │  │fileagent│  │  devmgr  │
               └────────┘  └────────┘  └──────────┘
```

**实现步骤**：

| 步 | 内容 | 技术要点 |
|----|------|---------|
| 1 | 定义 POSIX → CapsuleOS IPC 映射表 | 每个 POSIX 调用映射到一个 IPC opcode + 参数 |
| 2 | 实现文件操作 (`open`/`close`/`read`/`write`) | 转发到 `fileagent` |
| 3 | 实现进程操作 (`fork`/`exec`/`wait`/`exit`) | 转发到 `procmgr` |
| 4 | 实现内存操作 (`mmap`/`munmap`/`brk`) | 通过 `memmgr` (未来) 或直接 syscall |
| 5 | 实现时间操作 (`clock_gettime`/`nanosleep`) | 通过 `timed` 或内核 syscall |
| 6 | 实现 IOCTL / fcntl | 转发到 `devmgr` |
| 7 | 修改 `hnxlibc` 将 POSIX 调用改为 `posixd` IPC | 需要 `hnxlibc` 改造 |

**关键设计决定**：
- `posixd` = 无状态代理，所有实际功能委托给对应的后端服务
- 不实现完整的 POSIX，只实现使用到的子集 (文件IO + 进程 + 内存 + 信号)
- 进程 `fork` 通过 `procmgr` 创建子进程，`posixd` 只维护 PID 映射
- `exec` 通过 `procmgr` 加载新二进制 (由 `loader` 辅助)

---

## Phase 3：高可靠层

### 3.1 `reincar` — 重生服务器

**职责**：监控关键系统服务，在崩溃后自动重启。微内核环境中，任意用户态服务都可能故障 (`EL0-FAULT`)，`reincar` 是保证系统自愈的核心机制。

**架构**：
```
┌──────────┐  心跳/监控   ┌──────────┐
│  procmgr │ ←────────── │          │
├──────────┤             │ reincar  │
│  devmgr  │ ←────────── │  (EL0)   │
├──────────┤             │          │
│  logd    │ ←────────── │          │
├──────────┤             └────┬─────┘
│  ...     │                  │
└──────────┘          ┌───────┴───────┐
                      │    procmgr    │
                      │ (重启服务进程) │
                      └───────────────┘
```

**实现步骤**：

| 步 | 内容 | 技术要点 |
|----|------|---------|
| 1 | 创建 `userspace/services/reincar` 项目 | 无 |
| 2 | 定义服务注册协议：服务启动时向 `reincar` 注册 `(pid, name, restart_policy)` | IPC 消息 |
| 3 | 实现心跳机制：`reincar` 定期 Ping 注册的服务 | 定时 IPC |
| 4 | 实现崩溃检测：通过 `procmgr` 订阅进程退出事件 | 内核通知 `procmgr` → `reincar` |
| 5 | 实现重启策略：`Always` / `OnFailure` / `Once` | 可配置 |
| 6 | 实现重启计数和退避：防止反复崩溃导致无限重启 | 3 次后暂停 |

**重启流程**：
```
reincar 检测到 devmgr 崩溃
  → 通知 procmgr 释放 devmgr 资源
  → 通知 loader 重新加载 devmgr 二进制
  → devmgr 重新启动并注册到 reincar
  → 依赖 devmgr 的服务重新连接
```

---

### 3.2 `secmgr` — 安全管理器

**职责**：管理访问控制策略、能力 (capability) 委派、沙盒边界。微内核的安全核心。

**最小实现**（跳过复杂的 capability 系统）：

| 步 | 内容 | 技术要点 |
|----|------|---------|
| 1 | 定义安全策略格式：`(subject, object, permission)` | JSON 或简单二进制 |
| 2 | 实现 IPC 鉴权：`secmgr` 检查每个 IPC 调用是否被允许 | `devmgr` / `fileagent` 接入 |
| 3 | 实现沙盒：限制进程可以调用的 syscall 集合 | 内核侧 syscall filter |
| 4 | 实现安全策略文件加载：从 rootfs 读取 | 通过 `fileagent` |

---

## Phase 4：网络层

### 4.1 `netd` — 网络协议栈服务

**职责**：用户态 TCP/IP 协议栈，管理网络接口、路由、socket。

| 步 | 内容 | 技术要点 |
|----|------|---------|
| 1 | 选择协议栈实现：lwIP / smoltcp / 自研 | 推荐 `smoltcp` (Rust, no_std 友好) |
| 2 | 实现虚拟网卡驱动：通过 `devmgr` 访问 MMIO/中断 | QEMU `virtio-net` |
| 3 | 实现 socket API：`socket`/`bind`/`listen`/`accept`/`connect`/`send`/`recv` | IPC 接口 |
| 4 | 实现 DNS 解析 | 简单 stub + 配置文件 |
| 5 | 实现 DHCP 客户端 | 可选 |

---

## Phase 5：硬件抽象层

### 5.1 `timed` — 时间服务

| 步 | 内容 | 技术要点 |
|----|------|---------|
| 1 | 从内核获取当前时间 (通过 syscall 读取 `CNTPCT_EL0`) | 需新增 `SYS_CLOCK_GETTIME` |
| 2 | 提供 `clock_gettime` / `nanosleep` IPC | 给 `posixd` 和程序用 |
| 3 | 实现 RTC 同步 (从 QEMU 的 FDT/DTB 读取启动时间) | 一次性同步 |

### 5.2 `crashd` — 崩溃报告服务

| 步 | 内容 | 技术要点 |
|----|------|---------|
| 1 | 监听从 `procmgr` 来的进程异常退出事件 | 等待 procmgr |
| 2 | 收集崩溃上下文：PID、ELR、FAR、SPSR、堆栈前几层 | 内核通过 IPC 提供 |
| 3 | 写入崩溃日志到文件系统 | 通过 `fileagent` |
| 4 | 格式化为 `.ohcrash` 文件 | 文本格式 |

---

## 实施时间线总览

```
Phase 1 (立即可做)
  ├── ✅ devmgr (已有)
  ├── ✅ fileagent (已有)
  ├── ✅ loader (已有)
  ├── ✅ osh + 工具 (已有)
  ├── ⏳ procmgr (已规划，40h)
  └── 📋 logd (20h)

Phase 2 (procmgr 之后)
  ├── 📋 posixd (60h，最复杂)
  └── 📋 timed (8h)

Phase 3 (系统稳定后)
  ├── 📋 reincar (30h)
  └── 📋 secmgr (40h)

Phase 4 (基础设施就绪)
  └── 📋 netd (80h+)

Phase 5 (长期)
  ├── 📋 crashd (15h)
  ├── 📋 debugd (20h)
  ├── 📋 storaged (30h)
  └── 📋 powermgr (15h)
```

---

## 启动时序 (目标)

```
bootloader → kernel → devmgr → fileagent → loader
                                              │
                          ┌───────────────────┼───────────────────┐
                          ↓                   ↓                   ↓
                        logd               procmgr           (硬件驱动)
                          │                   │
                          └───────────────────┼───────────────────┘
                                              ↓
                                            osh (shell)
                                              │
                              ┌───────────────┼───────────────┐
                              ↓               ↓               ↓
                           posixd          timed           reincar
                              ↓               ↓               ↓
                          (用户程序)       (时间同步)       (服务监控)
```
