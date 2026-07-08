# OHLINK 工具链 (`ohlink-cc`)

`ohlink-cc` 是为 **HNX 操作系统** 和 **CapsuleOS** 提供的一个完整、纯 Rust 实现的 `OHLINK` 目标文件及可执行二进制文件格式工具链。

## 一、 项目背景及设计目标

在 CapsuleOS / HNX 微内核的设计中，传统的 ELF 格式存在多余元数据臃肿、解析逻辑复杂等局限，因此采用定制的 **OHLINK 二进制格式**。

本工具链为跨平台（特别是 ARM64 目标机）交叉编译环境提供支持，包括：
1. **`ohlink-format`**：支持 `no_std` 与 `std` 双环境的核心数据结构定义、读写 API，以及符合 IEEE 802.3 标准的 CRC32 校验库。
2. **`rustc_codegen_ohlink`**：一个定制的 `rustc` 编译器后端，实现 MIR $\rightarrow$ ARM64 机器码生成并输出 OHLINK 格式。
3. **`ohlink-linker`**：纯 Rust 开发的静态链接器，负责符号解析、重定位和段合并对齐。

---

## 二、 模块结构

- **`docs/OHLINK-SPEC.md`**: OHLINK 二进制规范定义文档。
- **`crates/ohlink-format`**:
  - `header.rs`: `OHLK_Header` (48 字节) 的底层多平台序列化与反序列化。
  - `entry.rs`: 段描述表项 `OHLK_Entry`，负责内存与文件数据的按对齐装载描述。
  - `symbol.rs`: 符号表项 `OHLK_Symbol` 读写。
  - `reloc.rs`: 包含 ARM64 架构下 6 种 P0 级核心重定位类型的定义。
  - `crc32.rs`: 纯 `no_std` 安全、零分配的 CRC32-IEEE 标准校验计算。
  - `builder.rs`: 安全构建 OHLINK 文件的 Builder。
  - `parser.rs`: 快速加载、解析和 CRC 校验的 Parser。
- **`crates/rustc_codegen_ohlink`**: 针对 `rustc` 的 Codegen 编译器插件。
- **`crates/ohlink-linker`**: OHLINK 静态链接器。

---

## 三、 安装与测试

要运行基础格式库的单元测试和校验测试，请使用：

```bash
cargo test -p ohlink-format
```

测试覆盖了：
- IEEE 802.3 标准 CRC32-IEEE 的正确性验证（针对标准序列 `"123456789"` 的 `0xCBF43926` 验证）。
- 运行时零堆分配的 OHLINK 段构建（`.text` 和 `.data` 段）与 Parser 结构反序列化回显验证。
