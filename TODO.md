# CapsuleOS 开发计划

> 目标: 快速出成果，先能用再完善

---

## Phase 1: v0.1.0 Pangu - 可启动内核 ⭐

> 目标: hnxcore.ohc 能启动并打印 "CapsuleOS v0.1.0"

### 1.1 stage1boot
- [ ] kernel/src/arch/aarch64/stage1.S (设置 SP，跳转内核)
- [ ] 更新 kernel/kernel.ld (stage1 入口点)

### 1.2 ohc-tool
- [ ] tools/ohc-tool/Cargo.toml
- [ ] tools/ohc-tool/src/main.rs
- [ ] 实现 .ohc 头部写入
- [ ] 实现 CRC32 校验
- [ ] 实现 payload 打包

### 1.3 kernel 入口
- [ ] kernel/src/lib.rs (_start, panic_handler)
- [ ] 打印 "CapsuleOS v0.1.0"
- [ ] 串口初始化

### 1.4 AArch64 架构
- [ ] boot.S (异常向量表)
- [ ] console.rs (PL011 UART)
- [ ] mmu.rs (段映射)

### 1.5 构建验证
- [ ] `cargo build --target aarch64-unknown-none -p kernel`
- [ ] `rust-lld -flavor gnu -T kernel.ld libkernel.a -o kernel.elf`
- [ ] `cargo run -p ohc-tool -- --input kernel.elf --output hnxcore.ohc`
- [ ] `qemu-system-aarch64 -kernel hnxcore.ohc` 成功启动

---

## Phase 2: v0.2.0 Pangu - 内存管理

### 2.1 MMU 分页
- [ ] 实现页表结构
- [ ] 实现分页内存分配
- [ ] VMO/VMAR 真正实现

### 2.2 物理页分配器
- [ ] buddy system 或 bitmap 分配器
- [ ] 物理页映射

---

## Phase 3: v0.3.0 Pangu - 多任务调度

### 3.1 调度器
- [ ] Round-robin 调度
- [ ] 线程创建/切换
- [ ] 时间片管理

### 3.2 进程管理
- [ ] Process 结构
- [ ] 进程创建/销毁

---

## Phase 4: v0.4.0 Pangu - IPC

### 4.1 Channel
- [ ] Channel 创建/读写
- [ ] Handle 传递

### 4.2 Port
- [ ] Port 消息队列
- [ ] 异步通知

---

## Phase 5: v0.5.0 Pangu - 用户态 devmgr

### 5.1 Device Manager
- [ ] devhost 服务
- [ ] 驱动加载框架

---

## Phase 6: v0.6.0 Pangu - POSIX 兼容层

### 6.1 POSIX API
- [ ] open/close/read/write
- [ ] fork/exec
- [ ] pipe/socket

---

## Phase 7: v0.7.0 Pangu - 文件系统服务

### 7.1 VFS
- [ ] 虚拟文件系统
- [ ] ramfs 实现

### 7.2 文件系统
- [ ] 简单文件系统驱动

---

## Phase 8: v0.8.0 Pangu - 网络栈服务

### 8.1 网络协议栈
- [ ] TCP/IP 基础
- [ ] 网络接口驱动

---

## Phase 9: v0.9.0 Pangu - 服务集成

### 9.1 系统集成
- [ ] 所有服务集成
- [ ] 启动流程完善

---

## Phase 10: v1.0.0 - 完整系统

### 10.1 capsule-os.img
- [ ] 系统打包工具 capsule-pack
- [ ] Shell 实现
- [ ] 根文件系统
- [ ] 包管理

---

## 开发规则

1. **内核极简**: 只做调度 + IPC + 内存
2. **DISPLAY 不在内核**: compositor 是用户空间服务
3. **HAL 抽象**: traits 定义在 hal/，实现放 kernel/src/arch/
4. **syscall handler 拆分**: 禁止巨大 match
5. **no_std**: 内核和 HAL 禁止 unsafe_code
6. **二进制格式**: 使用自定义 .ohc 格式

---

## 构建命令

```bash
# 开发构建
make build

# 发布构建
make kernel-release

# 打包 .ohc
make ohc

# 运行
make run

# 清理
make clean
```
