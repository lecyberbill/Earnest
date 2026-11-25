# shell-guard

Pure-Rust static shell command analyzer (POSIX `sh`/`bash` and Windows `PowerShell`/`cmd`) designed to guard AI agent terminal execution before spawning any subprocess.

- Pure Rust 2021, **zero external dependencies** (`std` only, `#![forbid(unsafe_code)]`).
- Never executes the inspected command: analysis is 100% static and deterministic.
- Never panics on adversarial inputs (unclosed quotes, truncated subshells, deep nesting, control bytes, or large payloads).

## Public API

```rust
use shell_guard::{inspect_command, RiskLevel, Verdict};

assert_eq!(
    inspect_command("ls -la"),
    Verdict::Allowed { risk: RiskLevel::ReadOnly }
);
assert!(inspect_command("vim notes.txt").is_blocked());
assert!(matches!(
    inspect_command("rm -rf /"),
    Verdict::DangerousBlocked { .. }
));
```

### Verdicts

| Verdict | Meaning |
| --- | --- |
| `Allowed { risk }` | Safe to execute; `RiskLevel::ReadOnly` or `RiskLevel::Mutating`. |
| `InteractiveBlocked { command, reason, suggestion }` | Command would block waiting for interactive human input; includes a concrete non-interactive alternative. |
| `DangerousBlocked { command, reason }` | Irreversible or host-destructive operation. |

## Pipeline

1. **`segment::split_segments`** — Quote-aware (`'...'` / `"..."`), escape-aware (`\` and PowerShell `` ` ``) and bounded `$()` subcommand splitter across `;`, `&&`, `||`, `|`, and newlines.
2. **`normalize::normalize`** — Unwraps command prefixes (`sudo`, `env`, `timeout`, `nice`, `nohup`, etc.) and embedded shell wrappers (`bash -lc`, `pwsh -Command`, `cmd /c`) while handling Windows paths and extensions case-insensitively.
3. **`rules::dangerous`** — Blocks catastrophic root deletions, disk formatting, raw block-device writes, host shutdown/reboot, remote pipe-to-shell (`curl ... | sh`), and fork bombs.
4. **`rules::interactive`** — Blocks editors, pagers, bare REPLs, unpiped `read`/`Read-Host`/`pause`, interactive SQL clients, `git commit` without `-m`, `npm init` without `-y`, `apt install` without `-y`, `tail -f`, `docker run -it`, etc.
5. **`rules::classify`** — Classifies allowed commands into `RiskLevel::ReadOnly` vs `RiskLevel::Mutating`.

## Tests

```bash
cargo test --manifest-path crates/shell-guard/Cargo.toml
```
