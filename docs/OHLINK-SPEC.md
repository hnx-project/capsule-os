# OHLINK 二进制文件格式规范 (Version 1.0)

本文档定义了 OHLINK 二进制格式。该格式专门用于 HNX 操作系统及 CapsuleOS 用户空间程序、引导加载程序和静态/动态链接文件。

---

## 一、 文件总体布局

一个完整的 OHLINK 可执行/目标文件由以下部分依次拼接构成：

1. **文件头 (OHLK_Header)**: 位于文件起始 0x0000 偏移处，固定 48 字节。
2. **段/Header表 (Header Table)**: 紧跟在文件头之后（起始偏移 0x0030），包含 $N$ 个 `OHLK_Entry`，每个条目 32 字节。
3. **数据区 (Data Sections)**: 存储各段的实际二进制数据（如 `.text`、`.data`、符号表、字符串表、重定位表等）。

```
偏移 (Offset)           内容 (Content)
─────────────────────────────────────────────────────────────────
0x0000                  OHLK_Header (48 字节)
0x0030                  Header Table (N × 32 字节)
                        ├─ Entry[0]: .text
                        ├─ Entry[1]: .data
                        ├─ Entry[2]: .rodata
                        ├─ Entry[3]: .bss
                        ├─ Entry[4]: 符号表 (Symbol Table)
                        ├─ Entry[5]: 字符串表 (String Table)
                        └─ ...
0x0030 + N * 32         数据区 (Data Block)
                        ├─ .text 实际机器码
                        ├─ .data 初始化全局变量数据
                        ├─ .rodata 只读数据
                        ├─ 符号表数组 (OHLK_Symbol 数组)
                        ├─ 字符串表 (以 '\0' 分隔的 UTF-8 字符串)
                        └─ 重定位表 (OHLK_Reloc 数组)
```

---

## 二、 核心数据结构

### 2.1 文件头 (OHLK_Header) - 48 字节

文件头包含格式魔数、版本信息、架构和文件全局元数据。采用标准字节对齐。

| 偏移 (Byte) | 类型 | 字段名 | 说明 |
| :--- | :--- | :--- | :--- |
| `0x00` - `0x03` | `uint32_t` | `magic` | 必须为 `"OHLK"` 的大端数值 = `0x4F484C4B` |
| `0x04` - `0x05` | `uint16_t` | `version_major` | 主版本号，目前为 `1` |
| `0x06` - `0x07` | `uint16_t` | `version_minor` | 次版本号，目前为 `0` |
| `0x08` | `uint8_t` | `endian` | 字节序。`0` = 小端 (Little Endian)，`1` = 大端 (Big Endian) |
| `0x09` | `uint8_t` | `arch` | 目标架构。`1` = ARM64, `2` = x86_64, `3` = RISC-V64 |
| `0x0A` - `0x0B` | `uint16_t` | `header_count` | Header Entry (段) 的条目数量 $N$ |
| `0x0C` - `0x0F` | `uint32_t` | `header_offset` | Header 表在文件中的起始偏移（通常为 `0x00000030`） |
| `0x10` - `0x13` | `uint32_t` | `data_offset` | 实际数据区起始偏移（通常为 `0x30 + N * 32`） |
| `0x14` - `0x1B` | `uint64_t` | `file_size` | 文件总大小（字节数） |
| `0x1C` - `0x1F` | `uint32_t` | `checksum` | 文件校验和：对除 `checksum` 字段本身（用 `0` 填充）以外的整个文件计算的 CRC32-IEEE 校验值 |
| `0x20` - `0x23` | `uint32_t` | `flags` | 位掩码。例如：Bit 0 = 是否为位置无关可执行文件 (PIE)，Bit 1 = 是否为动态链接库 (Shared) |
| `0x24` - `0x2F` | `uint8_t[12]`| `reserved` | 保留字段，默认用 `0` 填充 |

---

### 2.2 段描述条目 (OHLK_Entry) - 32 字节

每个 `OHLK_Entry` 描述一个物理或逻辑段，例如代码段、数据段或元数据段（如符号表、重定位表）。

| 偏移 (Byte) | 类型 | 字段名 | 说明 |
| :--- | :--- | :--- | :--- |
| `0x00` - `0x03` | `uint32_t` | `ty` | 段类型（详见后文段类型定义表） |
| `0x04` - `0x07` | `uint32_t` | `flags` | 段访问属性掩码。Bit 0 = 可读 (R), Bit 1 = 可写 (W), Bit 2 = 可执行 (X) |
| `0x08` - `0x0F` | `uint64_t` | `offset` | 该段数据在 OHLINK 文件中的绝对偏移量 |
| `0x10` - `0x17` | `uint64_t` | `file_size` | 该段在文件中的实际对齐大小（字节数） |
| `0x18` - `0x1F` | `uint64_t` | `mem_size` | 该段加载到内存中分配的实际大小。对于 `.bss` 段，`file_size` 为 0，而 `mem_size` > 0 |

#### 段类型 (Type) 定义表：

| 段类型值 (Hex) | 常量名称 | 含义与典型属性 |
| :--- | :--- | :--- |
| `0x00010001` | `TYPE_TEXT` | 代码段 `.text` (R+X) |
| `0x00020002` | `TYPE_DATA` | 已初始化数据段 `.data` (R+W) |
| `0x00030003` | `TYPE_RODATA` | 只读全局变量/常量段 `.rodata` (R) |
| `0x00040004` | `TYPE_BSS` | 未初始化数据段 `.bss` (R+W, `file_size` = 0) |
| `0x00100010` | `TYPE_SYMTAB` | 符号表段 (Symbol Table) |
| `0x00110011` | `TYPE_STRTAB` | 字符串表段 (String Table) |
| `0x00120012` | `TYPE_RELOC` | 重定位表段 (Relocation Table) |
| `0x00200020` | `TYPE_DYNAMIC` | 动态链接段 |
| `0x8000xxxx` | `TYPE_CUSTOM` | 自定义扩展段（未知的自定义段装载器可选择忽略） |

---

### 2.3 符号表项 (OHLK_Symbol) - 24 字节

符号表由一系列连续的 `OHLK_Symbol` 结构体构成。

| 偏移 (Byte) | 类型 | 字段名 | 说明 |
| :--- | :--- | :--- | :--- |
| `0x00` - `0x07` | `uint64_t` | `name_offset` | 符号名在字符串表 (String Table) 段中的相对偏移（以 0 开始的字节偏移） |
| `0x08` | `uint8_t` | `ty` | 符号类型。`0` = 未定义 (None), `1` = 函数 (Function), `2` = 数据 (Data), `3` = 文件 (File) |
| `0x09` | `uint8_t` | `binding` | 绑定属性。`0` = 局部 (Local), `1` = 全局 (Global), `2` = 弱引用 (Weak) |
| `0x0A` - `0x0B` | `uint16_t` | `section_idx` | 该符号所属段在 Header 表中的索引。若符号为外部未定义符号，则填 `0xFFFF` |
| `0x0C` - `0x13` | `uint64_t` | `value` | 符号值。若是定义在目标文件中的符号，指在该段内的相对偏移量；若已链接完毕，指其虚拟/绝对内存地址 |
| `0x14` - `0x17` | `uint32_t` | `size` | 符号占用的空间大小（字节数） |

---

### 2.4 重定位表项 (OHLK_Reloc) - 32 字节

当目标文件（`.ohlk` 目标文件）未完成链接时，代码或数据段中对其他符号的引用尚未绑定地址，需要通过重定位表项进行说明。

| 偏移 (Byte) | 类型 | 字段名 | 说明 |
| :--- | :--- | :--- | :--- |
| `0x00` - `0x07` | `uint64_t` | `offset` | 被重定位点在目标段内的字节偏移。例如：在 `.text` 中的第 24 字节处需要重定位 |
| `0x08` - `0x0B` | `uint32_t` | `ty` | 重定位类型。ARM64 支持 P0 级 6 种标准重定位类型（见后文） |
| `0x0C` - `0x0F` | `uint32_t` | `symbol_idx`| 所依赖的符号在符号表段中的索引 |
| `0x10` - `0x17` | `int64_t` | `addend` | 显式加数（Addend），用于 `Value + Addend` 修正 |
| `0x18` - `0x19` | `uint16_t` | `section_idx`| 被重定位点所在的段在 Header 表中的索引（如 `.text` 的索引） |
| `0x1A` - `0x1F` | `uint8_t[6]` | `reserved` | 预留对齐字节 |

#### ARM64 P0 重定位类型定义清单：

| 类型值 (Decimal) | 重定位符号名称 | 说明 | 计算公式 |
| :--- | :--- | :--- | :--- |
| `1` | `R_AARCH64_ABS64` | 64 位绝对地址直接修正 | `S + A` |
| `2` | `R_AARCH64_CALL26` | 26 位 BL / B 跳转指令偏移 | `(S + A - P) >> 2` |
| `3` | `R_AARCH64_ADR_PREL_PG_HI21` | ADRP 指令页基址跳转偏移 | `(Page(S + A) - Page(P)) >> 12` |
| `4` | `R_AARCH64_ADD_ABS_LO12_NC` | ADD 立即数低 12 位页内偏移 | `(S + A) & 0xFFF` |
| `5` | `R_AARCH64_LDST64_ABS_LO12_NC`| LDR/STR 64 位立即数加载/存储 | `((S + A) & 0xFFF) >> 3` |
| `6` | `R_AARCH64_LDST32_ABS_LO12_NC`| LDR/STR 32 位立即数加载/存储 | `((S + A) & 0xFFF) >> 2` |

> *公式变量说明：`S` = 目标符号的绝对物理/虚拟地址；`A` = 加数 `addend`；`P` = 被重定位点的绝对物理/虚拟地址；`Page(X)` = `X & !0xFFF`*

---

## 三、 校验和 (CRC32-IEEE) 规范

为了在 `no_std` 裸机（微内核加载器）和普通开发机上快速验证 OHLINK 文件的完整性，规定：
1. 文件头中的 `checksum` 必须使用标准 **CRC32-IEEE** 计算，其多项式为 `0xEDB88320`。
2. 校验和计算输入包括整个文件的所有字节。
3. **计算步骤**：
   - 构造文件缓冲，并将 `OHLK_Header` 中 `checksum` 字段所在的位置（`0x1C` 至 `0x1F` 偏移处的 4 字节）全部清零。
   - 对整个缓冲执行 CRC32 校验。
   - 将计算得到的 32 位 CRC32 结果写入 `checksum` 字段，完成文件保存。
