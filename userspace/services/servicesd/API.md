# 🛡️ Init & Service Manager (`svc.init`) - Module API Specification

## 状态 (Status)
*   **Status**: `Active (使用中)`
*   **Version**: `1.0.0-beta`

---

## 组件名称 (Name)
*   **Name**: `svc.init` (自愈与服务管理器 / Init & Service Manager - `initd`)

---

## 依赖/关联组件说明 (Dependencies & Related Components)
1.  **`loader`**: 系统上电后首个被微内核拉起的用户态自举引导进程。`loader` 变回极简状态，只负责加载并启动 `initd`，随后退出。
2.  **`libcapsule`**: 用户态原生系统调用桥梁，`initd` 及其子服务使用它来连接服务通道、发送和接受消息。
3.  **核心 L3 系统服务 (`blkdev`, `devmgr`, `procmgr`, `fileagent`, `tty`)**: 在系统引导期受 `initd` 的有向无环图（DAG）编排加载，并在完成各自初始化后，向 `svc.init` 回送就绪信号（Ready Handshake）。

---

## 核心职责与定义 (Core Definition)
`initd` 运行于 EL0 沙箱中，是整个 CapsuleOS 的 **1号进程**，承担着协调系统初始化、按顺序启动子系统、监控服务健康度并进行在线故障自愈的最高生命周期管理职责。

其核心职责包括：

1.  **声明式 DAG 并发顺序编排**:
    *   管理服务之间的依赖关系，支持并发加载无前置依赖的服务。
    *   在确保所有依赖项完全就绪后，才触发拉起下游子系统（例如：等待 `devmgr` 与 `blkdev` 就绪后启动 `fileagent`）。
2.  **状态就绪握手 (Ready Handshake)**:
    *   不仅“拉起”进程，而且必须建立“握手确认”。各子系统启动并创建对应的微内核全局系统服务（如 `svc.vfs`）后，通过向 `svc.init` 反馈 `INIT_CMD_READY` 数据包完成报到，`initd` 确认后才将该服务转入 `Running` 状态。
3.  **非阻塞退出回收与生命周期监控 (`wait4` Watcher)**:
    *   非阻塞式回收子进程资源。当有服务由于运行越界崩溃时，`initd` 能够即时捕获该退出状态（通过 PID 及退出码），并结合 DAG 依赖图，进行自愈。

---

## 暴露接口与公共约定 (Exposed Interfaces & Protocols)

### 1. 全局服务名称 (Global Service Registration)
服务启动后，将在微内核命名空间中发布全局服务：
*   **Service Name**: `svc.init`

### 2. 会话命令协议 (Session Protocol Packet Layout)
客户端各服务向 `initd` 发送就绪消息的会话通道遵循 **148 字节对齐固定帧数据包**：

*   **`[Offset 0]` (1 Byte)**: 指令码 (Command Code)。
*   **`[Offset 1]` (1 Byte)**: 序列号 (Sequence)。
*   **`[Offset 2..4]` (2 Bytes)**: 预留 (Reserved)。
*   **`[Offset 4..8]` (4 Bytes)**: 参数1 (例如：就绪服务的退出码、PID等)。
*   **`[Offset 20..148]` (128 Bytes)**: 数据负载 (Data Payload，如报到服务的英文服务标识符，例如 `"blkdev"` / `"fileagent"` 等)。

---

### 3. 指令集定义 (Command Codes)

| 指令码 | 常量名称 | 含义说明 | 负载与参数约定 |
| :---: | :--- | :--- | :--- |
| `0x01` | `INIT_CMD_READY` | 子服务宣告初始化完成，握手就绪 | **Payload**: 该服务的注册名称字符切片（如 `"fileagent"`）。<br>**返回**: `initd` 记录并转换其状态为 `Running`。 |
| `0x02` | `INIT_CMD_HEARTBEAT` | 定期探活双向保核心跳 | **Arg1**: `PID`。<br>**返回**: `initd` 记录其活跃心跳。 |
| `0x03` | `INIT_CMD_CRASH_NOTIFY` | 异常通知或主动注销 | **Arg1**: 退出代码。<br>**Payload**: 崩溃退出服务名称。 |
