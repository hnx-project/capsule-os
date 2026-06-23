# Capsule OS - 开发计划

> 目标: 快速出成果，先能用再完善

---

## Phase 0: 项目初始化 (Week 1) - ✅ 完成

### 0.1 项目结构 ✅
- [x] Workspace 配置 (Cargo.toml)
- [x] hal/, shared/, kernel/, userspace/, gui/ 目录
- [x] AGENTS.md, TODO.md, .gitignore

### 0.2 HAL traits ✅
- [x] hal/src/cpu.rs (Cpu, CpuInfo traits)
- [x] hal/src/mmu.rs (Mmu, PageTable traits)
- [x] hal/src/interrupt.rs (InterruptController trait)
- [x] hal/src/timer.rs (Timer trait)
- [x] hal/src/console.rs (Console trait)
- [x] hal/src/memory.rs (PhysicalMemory trait)

### 0.3 shared 类型 ✅
- [x] shared/src/status.rs (Status, Result)
- [x] shared/src/types.rs (Handle, HandleValue, ObjectType)
- [x] shared/src/ipc.rs (Message types)
- [x] shared/src/boot.rs (BootInfo)

### 0.4 kernel 骨架 ✅
- [x] kernel/src/lib.rs (入口, panic handler)
- [x] kernel/src/arch/mod.rs
- [x] kernel/src/task/mod.rs
- [x] kernel/src/mm/mod.rs
- [x] kernel/src/ipc/mod.rs
- [x] kernel/src/object/mod.rs
- [x] kernel/src/syscall/mod.rs
- [x] kernel/src/sync/mod.rs
- [x] kernel/src/kcore/mod.rs
- [x] kernel/kernel.ld (链接脚本)

### 0.5 构建验证 ✅
- [x] `cargo build --target aarch64-unknown-none -p kernel` 成功
- [x] `make kernel-release` 生成 kernel.elf

---

## Phase 1: 最小可启动系统 (Week 2-3) - 🔄 进行中

> 目标: QEMU 启动，能打印 "Hello World"

### 1.1 AArch64 架构实现
- [x] kernel/src/arch/aarch64/mod.rs (UART putchar)
- [ ] kernel/src/arch/aarch64/boot.S (启动汇编)
- [ ] kernel/src/arch/aarch64/mmu.rs (MMU 启用)
- [ ] kernel/src/arch/aarch64/linker.ld

### 1.2 X86_64 架构实现
- [ ] kernel/src/arch/x86_64/ (实现)

### 1.3 内核核心实现
- [ ] kcore/alloc.rs (堆分配器 - 需要实现)
- [ ] task/scheduler.rs (需要完善)
- [ ] task/thread.rs (线程切换)

### 1.4 基础 Syscall
- [ ] sys_exit, sys_write (完善)
- [ ] sys_channel_*
- [ ] sys_vmo_*

### 1.5 Init 进程
- [ ] userspace/services/init (完善)
- [ ] Init 打印 "Hello from userspace!"

### 1.6 QEMU 测试
- [x] QEMU 可以启动 kernel.elf
- [ ] 验证串口输出工作

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
- [ ] mm/phys.rs (物理页分配器)

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

## 构建命令

```bash
# 构建 kernel (开发版)
make build

# 构建 kernel (发布版)
make kernel-release

# 运行 QEMU
make run

# 清理
make clean

# 检查编译
make check
```

---

## 开发规则

1. **内核极简**: 只做调度 + IPC + 内存
2. **GUI 不在内核**: compositor 是用户空间服务
3. **HAL 抽象**: traits 定义在 hal/，实现放 kernel/src/arch/
4. **syscall handler 拆分**: 禁止巨大 match
5. **no_std**: 内核和 HAL 禁止 unsafe_code
