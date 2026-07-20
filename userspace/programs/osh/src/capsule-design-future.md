# osh 终极技术演进设计方案：ZLE 与 AST 解析器

本篇文档是针对 `osh` (Capsule OS Shell) 的中长期技术演进蓝图。为了在 `no_std`、**无动态堆内存分配（No-Heap Allocation）** 的极致硬件约束下，实现媲美 **`zsh`** 的丝滑交互体验与强大的脚本执行能力，我们详细规划了以下两大核心组件的设计与实现。

---

## 目录

1. [【方向一】自研无分配 TTY 行编辑器 (ZLE 雏形)](#1-方向一自研无分配-tty-行编辑器-zle-雏形)
   * 1.1 原始模式 (Raw Mode) 与 TTY 驱动重塑
   * 1.2 逐字符拦截状态机
   * 1.3 零堆分配的控制台字符刷新与重绘算法
   * 1.4 无分配 Tab 键前缀自动补全 (Autocomplete)
2. [【方向三】Zsh 级高阶脚本解析器与抽象语法树 (AST)](#2-方向三zsh-级高阶脚本解析器与抽象语法树-ast)
   * 2.1 无分配零拷贝词法分析器 (Lexer)
   * 2.2 树状流控制解析器 (AST Parser)
   * 2.3 流程控制指令（If, For, While）的无堆栈实现

---

## 1. 【方向一】自研无分配 TTY 行编辑器 (ZLE 雏形)

传统的 POSIX 终端默认处于**规范模式（Canonical Mode）**，输入被缓存在 TTY 驱动中，只有在按下回车（`\n`）时，Shell 才能一次性读取整行。这彻底阻断了实现“逐字高亮”、“Tab 实时补全”与“方向键搜寻历史”的可能性。

### 1.1 原始模式 (Raw Mode) 与 TTY 驱动重塑

要拦截每一个字符，我们必须向内核请求将 TTY/UART 驱动置为 **Raw Mode（原始模式）**，具体实现包括：
1. **关闭回显 (ECHO)**：用户键入的字符不会自动显示在屏幕上，回显完全由 `osh` 自主代理控制（这是实时字符变色和补全的前提）。
2. **关闭规范缓冲 (ICANON)**：输入的每一个字符在 UART 捕获到时，会立即被 `sys_read` 读取，不进行行缓冲。
3. **CapsuleOS 底层支撑**：
   在 `hnxlibc` / 内核中应提供一个配置 TTY 的系统调用（如极简版 `ioctl` 或配置 TTY 的 channel 通道）：
   ```rust
   // 改变 TTY 运行模式：0 = Canonical (默认), 1 = Raw
   pub fn set_tty_raw_mode(fd: usize, raw: bool) -> Result<(), Status>;
   ```

### 1.2 逐字符拦截状态机

在 Raw Mode 下，`osh` 不再调用大缓存的 `read_line`，而是进入逐字节读取交互循环。我们需要一个状态机来识别多字节的 **转义序列（Escape Sequences）**：

当捕获到 `\x1b` (Escape 字符) 时，表明后面可能跟随一个控制指令（如方向键）。

```
                        +----------------+
                        |   Idle (Normal)|
                        +----------------+
                                |
                        \x1b    |
                                v
                        +----------------+
                        |  Escape Issued |
                        +----------------+
                                |
                        '['     |
                                v
                        +----------------+
                        |      CSI       |
                        +----------------+
                         /    |    \    \
                  'A'   /  'B'|     \'C' \ 'D'
                       v      v      v    v
                    [Up]   [Down] [Right] [Left]
```

#### 零内存分配的历史命令缓冲区 (Ring Buffer)
```rust
pub struct HistoryBuffer<const N: usize, const L: usize> {
    buffer: [[u8; L]; N], // 环形静态二维数组，无需分配内存
    head: usize,
    tail: usize,
    count: usize,
}
```
当捕获到 `[Up]` 或 `[Down]` 时，状态机直接在 `HistoryBuffer` 中移动指针，并将缓冲区中的历史行复制到当前输入缓冲区中，实现平滑的历史命令回溯。

### 1.3 零堆分配的控制台字符刷新与重绘算法

不使用堆分配（没有 `String` 的拼接和动态扩容），对控制台的重绘需要通过 **ANSI 逃逸码（ANSI Escape Codes）** 手动向显示器发送像素/光标刷新指令。

我们维护一个固定大小的静态行缓冲区及光标指针位置：
```rust
pub struct LineEditor<const MAX_LEN: usize> {
    buffer: [u8; MAX_LEN],
    len: usize,
    cursor: usize,
}
```

* **插入字符 `X`**：
  1. 将光标 `cursor` 之后的字符在 `buffer` 内整体向后移动 1 字节（静态切片 `copy_within`）。
  2. 往 `buffer[cursor]` 写入 `X`。
  3. `cursor += 1; len += 1;`。
  4. 向终端发送：
     * `\x1b[s`：保存当前光标位置。
     * 写入光标后剩余的内容：`write_stdout(&buffer[cursor-1..len])`。
     * `\x1b[u`：恢复光标位置。
     * `\x1b[C`：光标向右移动 1 格。

* **退格字符 (BackSpace)**：
  1. 将光标后方的字符整体向前覆盖。
  2. 向终端发送：
     * `\x1b[D`：光标向左移 1 格。
     * `\x1b[s`：保存光标。
     * 写入刷新后的剩余字符加一个空格（擦除最后一个多余字符）。
     * `\x1b[u`：恢复光标。

### 1.4 无分配 Tab 键前缀自动补全 (Autocomplete)

1. 当用户键入 `\x09` (Tab 键) 时，`osh` 挂起当前的键盘输入循环。
2. 提取光标前方的最后一个单词（如 `ls`，或者一个路径 `sys`）。
3. 通过 `hnxlibc` VFS 对接通道向 `svc.vfs` 检索对应路径：
   * 采用固定的静态临时缓冲区读取目录项前缀。
   * 无动态堆分配匹配：利用静态迭代器，对遍历的文件名与前缀进行 `starts_with` 筛选。
4. 如果有唯一匹配，直接修改并合并行编辑缓冲区 `buffer`，并将多余的自动补全部分通过标准重绘协议在控制台回显。

---

## 2. 【方向三】Zsh 级高阶脚本解析器与抽象语法树 (AST)

目前的命令行解析是通过简单空格拆分。要具备 Zsh 运行脚本和处理复杂指令组合的能力，`osh` 必须演进出一个前瞻性的、支持多行及控制流的高级解析器，并且需要完全克服 **`no_std` 下禁止频繁内存分配、禁止内存碎片化** 的底座要求。

### 2.1 无分配零拷贝词法分析器 (Lexer)

传统的词法分析会将分割出来的词转化成 `String`、打包进 `Vec<Token>`，在嵌入式或裸机操作系统早期极易发生 **内存堆耗尽（OOM）**。

#### 解决方案：基于借用与生命周期的 `Token<'a>`
`osh` 的词法分析器不持有任何数据，仅持有对输入缓冲区（`&'a str`）的 **只读借用（Borrow）** 与其字符边界索引：

```rust
#[derive(Debug, Clone, Copy)]
pub enum TokenKind {
    Word,              // 命令、路径或普通参数
    Pipe,              // |
    RedirectWrite,     // >
    RedirectAppend,    // >>
    And,               // &&
    Or,                // ||
    Semicolon,         // ;
    KeywordIf,         // if
    KeywordThen,       // then
    KeywordElse,       // else
    KeywordFi,         // fi
}

#[derive(Debug, Clone, Copy)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub lexeme: &'a str, // 借用，无堆内存分配
}
```

#### 静态双向 Token 迭代器
实现 `Iterator` 特征的 `Lexer<'a>` 每次只解析出下一个 Token，不进行预先全部分割，这把运行时空间复杂度直接从 `O(N)` 降为极其惊艳的 **`O(1)`** 级。

### 2.2 树状流控制解析器 (AST Parser)

为了保证多级管道、条件判断的安全拓扑执行，我们需要定义 AST（抽象语法树）。
在 `no_std` 下，传统的“用指针构造树并不断 `Box::new`” 的经典方式在裸机上由于缺乏堆分配器而无法通过编译。

#### 解决方案：基于内存平坦化的静态 Arena (树) 设计
我们定义一个固定大小的静态内存节点数组，通过 **索引（Index）** 代替传统的智能指针，来链接父子节点，实现一套平坦的、无动态开销的高性能 AST 树：

```rust
pub enum AstNodeKind<'a> {
    Command {
        name: &'a str,
        args: [&'a str; 8],
        arg_count: usize,
    },
    Pipeline {
        left_idx: usize,
        right_idx: usize,
    },
    Redirect {
        cmd_idx: usize,
        file: &'a str,
        append: bool,
    },
    IfStmt {
        cond_idx: usize,
        then_idx: usize,
        else_idx: Option<usize>,
    },
}

pub struct AstNode<'a> {
    pub kind: AstNodeKind<'a>,
}

// 静态平坦化的抽象语法树：
pub struct AstArena<'a, const MAX_NODES: usize> {
    pub nodes: [Option<AstNode<'a>>; MAX_NODES],
    pub count: usize,
}
```

通过这套内存平坦化（Flat-Arena）设计，解析器可以将任意复杂的 `cmd1 | cmd2 && cmd3` 或 `if-then-else` 语法全部打平并无碎损地存储在栈上，提供高安全度的解释运行！

### 2.3 流程控制指令（If, For, While）的无堆栈实现

在解释运行 `IfStmt` 时：
1. **解释执行条件节点（`cond_idx`）**：
   * 执行 `cond_idx` 指向的 AST 子树（通常为 `test -f /init`）。
   * 捕获其退出状态码。
2. **逻辑分流**：
   * 如果退出状态码为 0（代表条件成立/True），递归地去执行 `then_idx` 指向的 AST 分支。
   * 如果非 0，且 `else_idx` 有值，执行 `else_idx` 分支。
3. **优势**：
   * 递归过程在 Rust 编译出来的物理栈帧中直接调度，**整个解析执行过程不需要任何额外的堆栈内存分配**，兼顾了高可拓展性与惊艳的轻量级！
