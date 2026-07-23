# 🌐 Network Service (`svc.net`) - Module API Specification

## 状态 (Status)
*   **Status**: `Active (使用中)`
*   **Version**: `1.0.0-beta`

---

## 组件名称 (Name)
*   **Name**: `svc.net` (网络服务守护进程 / Network Service Daemon)

---

## 依赖/关联组件说明 (Dependencies & Related Components)
1.  **`libcapsule`**: 用户态标准库与核心系统调用中介。`svc.net` 通过 `libcapsule` 进行底层 Channel 读写，注册服务，并发送 `libcapsule::notify_init` 准备绪信号。
2.  **`initd`**: 系统服务管理器。`netd` 启动后会向其通知就绪，且接受其自动恢复与健康检查监管。
3.  **`kernel`**: 内核层。`netd` 通过系统调用进行底层以太网包（Ethernet Frames）的收发。

---

## 核心职责与定义 (Core Definition)
`svc.net` 运行于 EL0 沙箱中，负责在用户态实现整个 TCP/IP 协议栈。它接收来自其他用户态应用（如终端、网络测试程序）的 Socket 建立与连接请求，利用内嵌的 `smoltcp` 协议栈管理连接状态与窗口流控，并将数据包转换为标准的以太网帧，底座下沉到内核的物理收发接口。

主要职责包含：
1.  **用户态协议栈 (Userspace TCP/IP)**:
    *   处理 ARP 地址解析协议。
    *   处理 IPv4 封包、路由分发与 ICMP (Ping)。
    *   在 EL0 侧管理完整的 TCP 滑动窗口协议、三次握手、重传及状态转移。
2.  **套接字抽象 (Socket Abstraction)**:
    *   为其他进程提供虚拟的 `Socket Fd` (Socket 句柄) 概念和接口。
3.  **零阻塞/高性能包中介**:
    *   使用 148 字节对齐的固定会话帧，消除 IPC 变长包分配开销。

---

## 暴露接口与公共约定 (Exposed Interfaces & Protocols)

### 1. 全局服务名称 (Global Service Registration)
*   **Service Name**: `svc.net`

### 2. 会话命令协议 (Session Protocol Packet Layout)
客户端与 `svc.net` 的会话通道遵循 **148 字节对齐固定帧数据包**，其具体帧结构如下：

#### A. 帧头与指令格式
*   **`[Offset 0]` (1 Byte)**: 指令码 (Command Code)。
*   **`[Offset 1]` (1 Byte)**: 序列号 (Sequence Number, 用于异步并发匹配)。
*   **`[Offset 2..4]` (2 Bytes)**: 预留填充 (Reserved)。
*   **`[Offset 4..8]` (4 Bytes)**: 参数1 (例如：`SocketId` 局部句柄、Ioctl 类型等)。
*   **`[Offset 8..12]` (4 Bytes)**: 参数2 (例如：读取/写入大小、端口号 `Port`、协议类型 `TCP=1`/`UDP=2` 等)。
*   **`[Offset 12..20]` (8 Bytes)**: IPv4 地址 (IPv4 Address, 源或目标 IP，占 4 字节，后 4 字节预留)。
*   **`[Offset 20..148]` (128 Bytes)**: 数据负载 (Data Payload，如发送/接收的数据，最大 128 字节)。

---

### 3. 指令集定义 (Command Codes)

| 指令码 | 常量名称 | 含义说明 | 负载与参数约定 |
| :---: | :--- | :--- | :--- |
| `0x10` | `NET_CMD_SOCKET` | 创建并分配一个新的套接字 | **Arg2**: 协议类型 (`TCP=1` / `UDP=2`)。<br>**返回**: 响应帧携带服务分配的局部套接字句柄 `SocketId`。 |
| `0x11` | `NET_CMD_BIND` | 绑定 IP 与本地端口 | **Arg1**: `SocketId`。<br>**Arg2**: `Port`。<br>**Addr**: 目标 IP。 |
| `0x12` | `NET_CMD_LISTEN` | 开启 TCP 被动监听状态 | **Arg1**: `SocketId`。<br>**Arg2**: 最大待处理连接队列长度。 |
| `0x13` | `NET_CMD_ACCEPT` | 阻塞式接受外部连接 | **Arg1**: `SocketId`。<br>**返回**: 成功时返回一个新的套接字句柄。 |
| `0x14` | `NET_CMD_CONNECT` | 主动发起 TCP 三次握手 | **Arg1**: `SocketId`。<br>**Arg2**: 目标 `Port`。<br>**Addr**: 远端 IP 地址。 |
| `0x15` | `NET_CMD_SEND` | 发送网络数据包 | **Arg1**: `SocketId`。<br>**Arg2**: 数据长度 (Max 128)。<br>**Payload**: 发送的数据。 |
| `0x16` | `NET_CMD_RECV` | 读取已接收的套接字缓冲区 | **Arg1**: `SocketId`。<br>**Arg2**: 最大读取长度。<br>**返回**: Payload 携带网络数据，Arg2 携带实际读取字节。 |
| `0x17` | `NET_CMD_CLOSE` | 优雅拆除连接并释放套接字 | **Arg1**: `SocketId`。<br>**返回**: Status 状态码。 |
