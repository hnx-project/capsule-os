# ⌨️ TTY & Console Service (`svc.tty`) - Module API Specification

## 状态 (Status)
*   **Status**: `Active (使用中)`
*   **Version**: `1.0.0-beta`

---

## 组件名称 (Name)
*   **Name**: `svc.tty` (终端与 TTY 控制台服务 / Terminal & TTY Console Service)

---

## 依赖/关联组件说明 (Dependencies & Related Components)
1.  **`libcapsule`**: 用户态标准库与核心系统调用中介。`svc.tty` 通过 `libcapsule` 进行底层 Channel 创建、注册、服务拉起、串口读写和中断响应。
2.  **`devmgr` (`svc.dev`)**: 用于发现并接管底层的物理串口驱动设备（如 `pl011` 串口或键盘/显示器帧缓冲区）。
3.  **`procmgr` (`svc.proc`)**: 用于前台进程组管理与作业控制（Job Control）。当 TTY 拦截到强杀或中断热键（如 `Ctrl+C`、`Ctrl+Z`）时，调用 `procmgr` 向对应的前台进程组分发信号。
4.  **`osh` / `libc`**: 标准 POSIX 客户端。任何从 `osh` 拉起的用户进程在默认情况下，其 Fds 0, 1, 2 将重定向映射至 `svc.tty` 的会话通道。

---

## 核心职责与定义 (Core Definition)
`svc.tty` 运行于 EL0 沙箱中，是硬件控制台设备与用户态多任务 Shell（如 `osh`）之间的桥梁。它彻底解除了微内核对 POSIX 终端特定属性（如行规、控制转义符）的硬编码依赖。其主要职责包含：

1.  **行规规范化 (Line Discipline - Cooked Mode)**:
    *   在熟模式（Cooked Mode，默认状态）下缓冲用户的每一次击键，本地处理退格（Backspace, `0x08`/`0x7f`），只有在用户输入回车键（`\n` / `\r`）时才向读取方（如 `osh`）输送完整一行的内容。
    *   支持终端本地回显（Echo）。
2.  **原始透传模式 (Raw Mode)**:
    *   提供无回显、无缓冲的按键透传，供全屏应用（如文本编辑器、交互式游戏）直接消费高频键盘扫描流。
3.  **控制字符热键拦截**:
    *   实时监控键入字符：
        *   `Ctrl+C` (`0x03`): 拦截并触发向当前 TTY 绑定的前台进程组发送 `SIGINT` (强杀信号)。
        *   `Ctrl+D` (`0x04`): 翻译为输入流的 `EOF`，向读取方返回 0 字节代表流结束。
        *   `Ctrl+Z` (`0x1a`): 拦截并发送 `SIGTSTP` (后台挂起信号)。
4.  **物理输出翻译**:
    *   在向物理串口输出时，自动将单个换行符 `\n` 自动扩展为 `\r\n`（Carriage Return + Line Feed），确保传统串口终端工具排版显示正常。

---

## 暴露接口与公共约定 (Exposed Interfaces & Protocols)

### 1. 全局服务名称 (Global Service Registration)
服务启动后，将在微内核命名空间中发布全局 IPC 服务名：
*   **Service Name**: `svc.tty`

### 2. 会话命令协议 (Session Protocol Packet Layout)
客户端与 `svc.tty` 的会话通道遵循 **148 字节对齐固定帧数据包**（与 `fileagent` 统一），其具体帧结构如下：

#### A. 帧头与指令格式
*   **`[Offset 0]` (1 Byte)**: 指令码 (Command Code)。
*   **`[Offset 1]` (1 Byte)**: 序列号 (Sequence Number, 用于异步并发匹配)。
*   **`[Offset 2..4]` (2 Bytes)**: 预留填充 (Reserved)。
*   **`[Offset 4..8]` (4 Bytes)**: 参数1 (例如：读取最大字节数、Ioctl 类型等)。
*   **`[Offset 8..12]` (4 Bytes)**: 参数2 (例如：会话绑定 PID)。
*   **`[Offset 12..20]` (8 Bytes)**: 预留 (Reserved)。
*   **`[Offset 20..148]` (128 Bytes)**: 数据负载 (Data Payload，如写入的字符串、Ioctl 配置结构体)。

---

### 3. 指令集定义 (Command Codes)

| 指令码 | 常量名称 | 含义说明 | 负载与参数约定 |
| :---: | :--- | :--- | :--- |
| `0x01` | `TTY_CMD_READ` | 阻塞/非阻塞读取 TTY 熟字符 | **Arg1**: 最大读取长度。<br>**返回**: 响应帧携带已读字节数，Payload 为 Cooked 字符串。若无输入则服务阻塞当前读取通道。 |
| `0x02` | `TTY_CMD_WRITE` | 物理输出字符并翻译换行 | **Arg1**: 写入数据长度。<br>**Payload**: 要输出的数据。<br>**返回**: 响应帧携带实际写入字节数。 |
| `0x03` | `TTY_CMD_IOCTL` | 动态配置终端属性参数 | **Arg1**: `Ioctl` 命令类型。<br>**Payload**: `Termios` 结构体配置数据（如 Echo 开启、Raw/Cooked 切换）。 |
| `0x04` | `TTY_CMD_BIND_PGID` | 绑定前台进程组 (Job Control) | **Arg1**: 目标前台进程的 `PID` / `PGID`。<br>**返回**: 绑定成功状态，后续拦截到的中断热键将只打向该 `PGID`。 |

---

### 4. `Termios` 终端属性配置结构 (Ioctl Payload Layout)
当调用 `TTY_CMD_IOCTL` 时，负载 `[Offset 20..148]` 部分前 16 字节被解释为如下 `Termios` 结构：

```rust
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Termios {
    pub c_iflag: u32,  // 输入模式控制位 (如 IXON, ICRNL)
    pub c_oflag: u32,  // 输出模式控制位 (如 ONLCR 换行转换)
    pub c_cflag: u32,  // 控制模式位
    pub c_lflag: u32,  // 本地模式位 (如 ECHO, ICANON 行缓冲熟模式)
}
```

*   **`c_lflag` 核心掩码**:
    *   `ECHO` (`0x00000008`): 如果置位，用户敲入的任何字符将被立刻回显写回终端。
    *   `ICANON` (`0x00000002`): 如果置位，开启标准 Cooked 行规模式；若清除，进入 Raw 透传模式。
*   **`c_oflag` 核心掩码**:
    *   `ONLCR` (`0x00000004`): 如果置位，自动转换 `\n` 为 `\r\n`。
