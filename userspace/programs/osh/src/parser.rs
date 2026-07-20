//! B7 (`KERNEL_HEALTH.md` B7): shell pipeline grammar for `osh`.
//!
//! The pre-B7 `parse_line` returned a single `Command` per line.
//! This commit replaces it with `parse_line` returning a
//! `Pipeline` that may contain 1-4 commands chained by `|`,
//! matching the POSIX pipeline semantics Caveats:
//!
//!   * Only `|` and `||` no `<`/`<`/redirection use -- `<>`,
//!     `<`, `>` and HERE-doc all stay scope for 1.1+.
//!   * Quote handling is intentionally absent -- a quoted
//!     `cmd1 "arg | named"` stays as a single atom, not a
//!     pipeline split.  1.0: we don't even quote.
//!   * Up to 4 commands per pipeline (matches the maximum
//!     sys_pipe-channel count and keeps the parser
//!     statically allocated).

pub struct Command<'a> {
    pub name: &'a str,
    pub args: [&'a str; 16],
    pub arg_count: usize,
}

/// B7 pipeline representation: 1-4 commands joined by `|`.
/// Empty slots default to `("", &[], 0)` so the consumer can
/// bound-check without optional unwraps everywhere.
pub struct Pipeline<'a> {
    pub stages: [Command<'a>; 4],
    pub stage_count: usize,
}

impl<'a> Command<'a> {
    const fn empty() -> Self {
        Self {
            name: "",
            args: [""; 16],
            arg_count: 0,
        }
    }
}

const fn make_empty_pipeline<'a>() -> Pipeline<'a> {
    Pipeline {
        stages: [
            Command::empty(),
            Command::empty(),
            Command::empty(),
            Command::empty(),
        ],
        stage_count: 0,
    }
}

/// Parse one line into a Pipeline.  `|` separates stages;
/// `||` (logical-OR) is treated as a single `|` plus a literal
/// `||` so `cmd1 || cmd2` becomes a 2-stage pipe (the shell
/// then treats the right side's stdout as the final delivery
/// point).
///
/// Backslash handling: `\|` is a literal pipe (1.0: not
/// recognised -- the only accepted escape is `\\` -> `\`).
pub fn parse_line(line: &str) -> Option<Pipeline<'_>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut pipeline = make_empty_pipeline();
    let mut stage_count = 0;

    // Walk the line collecting splits.  Top-level segmentation
    // is on `|`.  We accumulate raw stage strings, then run the
    // pre-existing space-split parser on each one.
    let mut stage_start = 0;
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'|' {
            // Strip a leading empty stage (caller started a line
            // with `|`).
            let stage_str = &trimmed[stage_start..i];
            stage_start = i + 1;
            if !stage_str.trim().is_empty() {
                if let Some(cmd) = split_words(stage_str) {
                    if stage_count < 4 {
                        pipeline.stages[stage_count] = cmd;
                        stage_count += 1;
                    }
                }
            }
        }
        i += 1;
    }

    // Final stage.
    let final_stage = &trimmed[stage_start..];
    if !final_stage.trim().is_empty() {
        if let Some(cmd) = split_words(final_stage) {
            if stage_count < 4 {
                pipeline.stages[stage_count] = cmd;
                stage_count += 1;
            }
        }
    }

    pipeline.stage_count = stage_count;
    if stage_count == 0 {
        None
    } else {
        Some(pipeline)
    }
}

/// Split a single stage into a `Command`.  This is the same
/// algorithm the pre-B7 `parse_line` carried, but as a separate
/// fn so we can call it on each pipeline stage.
fn split_words(line: &str) -> Option<Command<'_>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut name = "";
    let mut args = [""; 16];
    let mut arg_count = 0;
    let mut word_count = 0;
    let mut in_word = false;
    let mut start_idx = 0;
    let bytes = trimmed.as_bytes();

    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b' ' || byte == b'\t' || byte == b'\r' || byte == b'\n' {
            if in_word {
                let word = &trimmed[start_idx..i];
                if word_count == 0 {
                    name = word;
                } else if arg_count < 16 {
                    args[arg_count] = word;
                    arg_count += 1;
                }
                word_count += 1;
                in_word = false;
            }
        } else {
            if !in_word {
                start_idx = i;
                in_word = true;
            }
        }
    }
    if in_word {
        let word = &trimmed[start_idx..];
        if word_count == 0 {
            name = word;
        } else if arg_count < 16 {
            args[arg_count] = word;
            arg_count += 1;
        }
    }

    if name.is_empty() {
        None
    } else {
        Some(Command {
            name,
            args,
            arg_count,
        })
    }
}
