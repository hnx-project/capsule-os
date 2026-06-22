# Capsule OS - 开发计划

> 目标: 快速出成果，先能用再完善

---

## 项目结构

```
capsule-os/
├── Cargo.toml              # Workspace 根配置
├── hal/                    # Hardware Abstraction Layer (no_std)
│   ├── src/
│   │   ├── lib.rs
│   │   ├── cpu.rs
│   │   ├── mmu.rs
│   │   ├── interrupt.rs
│   │   ├── timer.rs
│   │   ├── console.rs
│   │   └── memory.rs
│   └── Cargo.toml
│
├── shared/                 # 共享类型 (no_std)
│   ├── src/
│   │   ├── lib.rs
│   │   ├── status.rs
│   │   ├── types.rs
│   │   ├── ipc.rs
│   │   └── boot.rs
│   └── Cargo.toml
│
├── kernel/                 # 微内核 (no_std)
│   ├── src/
│   │   ├── lib.rs          # 内核入口, panic handler
│   │   ├── arch/
│   │   │   ├── mod.rs
│   │   │   ├── aarch64/
│   │   │   └── x86_64/
│   │   ├── task/
│   │   ├── mm/
│   │   ├── ipc/
│   │   ├── object/
│   │   ├── syscall/
│   │   ├── sync/
│   │   └── kcore/
│   └── Cargo.toml
│
├── userspace/              # 用户空间
│   ├── libc/              # Syscall 包装
│   ├── services/
│   │   ├── vfs/
│   │   ├── loader/
│   │   └── init/
│   └── programs/
│       └── shell/
│
├── gui/                    # 桌面环境
│   ├── compositor/
│   ├── renderer/
│   └── client/
│
└── TODO.md
```

---

## Phase 0: 项目初始化 (Week 1) - 进行中

### 0.1 创建项目结构 ✅
- [x] 创建目录结构
- [x] 初始化 Cargo workspace 配置
- [x] 创建 hal/, shared/, kernel/ 骨架
- [x] 创建 userspace/ 骨架
- [x] 创建 gui/ 骨架

### 0.2 创建 HAL traits ✅
- [x] hal/src/cpu.rs
- [x] hal/src/mmu.rs
- [x] hal/src/interrupt.rs
- [x] hal/src/timer.rs
- [x] hal/src/console.rs
- [x] hal/src/memory.rs

### 0.3 创建 shared 类型 ✅
- [x] shared/src/status.rs
- [x] shared/src/types.rs
- [x] shared/src/ipc.rs
- [x] shared/src/boot.rs

### 0.4 创建 kernel 骨架 🔄
- [x] kernel/src/lib.rs
- [x] kernel/src/arch/mod.rs
- [x] kernel/src/task/mod.rs
- [x] kernel/src/mm/mod.rs
- [x] kernel/src/ipc/mod.rs
- [x] kernel/src/object/mod.rs
- [x] kernel/src/syscall/mod.rs
- [x] kernel/src/sync/mod.rs
- [x] kernel/src/kcore/mod.rs

### 0.5 验证构建 ⚠️
- [ ] kernel 编译通过
- [ ] userspace 编译通过
- [ ] Makefile 工作

---

## Phase 1: 最小可启动系统 (Week 2-3)

> 目标: QEMU 启动，能打印 "Hello World"

### 1.1 AArch64 架构实现
- [ ] kernel/src/arch/aarch64/boot.S
- [ ] kernel/src/arch/aarch64/mmu.rs (启用 MMU)
- [ ] kernel/src/arch/aarch64/int.rs
- [ ] kernel/src/arch/aarch64/timer.rs
- [ ] kernel/src/arch/aarch64/console.rs (UART)
- [ ] kernel/src/arch/aarch64/linker.ld

### 1.2 X86_64 架构实现
- [ ] kernel/src/arch/x86_64/boot.S
- [ ] kernel/src/arch/x86_64/mmu.rs
- [ ] kernel/src/arch/x86_64/console.rs

### 1.3 内核核心实现
- [ ] kcore/alloc.rs (堆分配器)
- [ ] task/scheduler.rs (round-robin)
- [ ] task/thread.rs (线程切换)
- [ ] mm/phys.rs (物理页分配)

### 1.4 基础 Syscall
- [ ] sys_exit, sys_write
- [ ] sys_channel_create/read/write
- [ ] sys_vmo_create/read/write

### 1.5 Init 进程
- [ ] userspace/services/init
- [ ] Init 打印 "Hello from userspace!"

### 1.6 验证
- [ ] QEMU 启动成功
- [ ] 能看到 "Hello from userspace!"

---

## Phase 2: 基础 IPC 和进程管理 (Week 4-5)

### 2.1 IPC 机制
- [ ] ipc/channel.rs 完整实现
- [ ] ipc/port.rs 完整实现

### 2.2 进程/线程 Syscall
- [ ] sys_process_*
- [ ] sys_thread_*

### 2.3 内存管理
- [ ] mm/vmo.rs (真正分配物理页)
- [ ] mm/vmar.rs (页表映射)

### 2.4 ELF 加载
- [ ] mm/elf.rs
- [ ] userspace/services/loader/

---

## Phase 3: 简单 GUI ⭐ (Week 6-10)

### 3.1 显示驱动
- [ ] VirtIO GPU 驱动
- [ ] 帧缓冲

### 3.2 GUI 服务
- [ ] gui/compositor (窗口合成器)
- [ ] gui/renderer (2D 渲染)
- [ ] gui/client (客户端库)

### 3.3 输入处理
- [ ] 键盘驱动
- [ ] 鼠标驱动

### 3.4 窗口管理器
- [ ] 浮动窗口
- [ ] 焦点管理

### 3.5 验证
- [ ] QEMU 显示桌面
- [ ] 窗口可打开/关闭

---

## Phase 4: 桌面应用 (Week 11-14)

### 4.1 VFS 服务
### 4.2 文件管理器
### 4.3 终端模拟器
### 4.4 任务栏

---

## Phase 5: 完善 (Week 15+)

### 5.1 图形增强
### 5.2 网络支持
### 5.3 声音支持
### 5.4 持久化存储

---

## 开发规则

1. **内核极简**: 只做调度 + IPC + 内存
2. **GUI 不在内核**: compositor 是用户空间服务
3. **HAL 抽象**: traits 定义在 hal/，实现放 kernel/src/arch/
4. **syscall handler 拆分**: 禁止巨大 match
5. **no_std**: 内核和 HAL 禁止 unsafe_code
