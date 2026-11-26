# expr_eval

A secure, deterministic, **zero-dependency** expression evaluator for business rules and workflows, written in Rust (edition 2021, std only).

## Features

- **Typed values**: `null`, `bool`, `int` (i64), `float` (f64), `str`, `list`, `map`
- **Safe arithmetic**: overflow-checked integer operations, division-by-zero rejection
- **Short-circuit evaluation**: `&&`, `||`, and `? :` never evaluate the unused branch
- **Recursion limit**: expressions deeper than 64 levels are rejected at parse time (anti-DoS)
- **Step budget**: configurable execution cap (default 10 000 steps) prevents runaway evaluations
- **Zero dependencies**: no external crates, `#![forbid(unsafe_code)]`
- **Deterministic**: `BTreeMap` for maps, no floating-point nondeterminism in control flow

## Quick Start

```rust
use expr_eval::{compile, EvalContext, Value};

let expr = compile("x > 5 && name starts_with 'Ad'").unwrap();

let mut ctx = EvalContext::new();
ctx.set_variable("x", Value::Int(10));
ctx.set_variable("name", Value::Str("Admin".into()));

let result = expr.eval(&ctx).unwrap();
assert_eq!(result, Value::Bool(true));
```

## Syntax

### Literals

| Type    | Examples                          |
|---------|-----------------------------------|
| null    | `null`                            |
| bool    | `true`, `false`                   |
| int     | `42`, `-7`                        |
| float   | `3.14`, `-0.5`                    |
| string  | `"hello"`, `'it\'s'`              |
| list    | `[1, 2, 3]`, `["a", "b"]`        |
| map     | *(see native construction)*       |

### Operators (precedence, low → high)

| Precedence | Operators                          |
|------------|------------------------------------|
| 1 (lowest) | `? :` (ternary)                    |
| 2          | `||`                               |
| 3          | `&&`                               |
| 4          | `==` `!=`                          |
| 5          | `<` `<=` `>` `>=`                  |
| 6          | `contains` `starts_with` `ends_with` `in` |
| 7          | `+` `-`                            |
| 8          | `*` `/` `%`                        |
| 9          | unary `!` `-`                      |
| 10 (highest)| postfix `.` `[]` `()`            |

### String escapes

`\"` `\'` `\\` `\n` `\t`

### Member / index access

```
user.name
user.roles[1]
items[0].name
```

### Ternary

```
condition ? then_expr : else_expr
```

## Built-in Functions

| Function           | Description                              |
|--------------------|------------------------------------------|
| `len(x)`           | Length of string, list, or map           |
| `lower(s)`         | Uppercase a string                       |
| `upper(s)`         | Lowercase a string                       |
| `trim(s)`          | Trim whitespace from a string            |
| `abs(n)`           | Absolute value of a number               |
| `min(a, b)`        | Minimum of two numbers                   |
| `max(a, b)`        | Maximum of two numbers                   |
| `coalesce(...)`    | First non-null argument                  |

## Native Functions

Register host-defined functions:

```rust
use std::sync::Arc;
use expr_eval::{EvalContext, Value, ValueError};

let mut ctx = EvalContext::new();
ctx.register_function("greet", Arc::new(|args: Vec<Value>| -> Result<Value, ValueError> {
    match args.first() {
        Some(Value::Str(s)) => Ok(Value::Str(format!("Hello, {s}!"))),
        _ => Err(ValueError::TypeMismatch { expected: "str".into(), found: "?".into() }),
    }
}));
```

## Step Budget

```rust
let mut ctx = EvalContext::new();
ctx.set_max_steps(1000); // reject evaluation after 1000 AST node visits
```

## Error Handling

All errors carry source position (`offset`, `line`, `col`) for parse errors:

```rust
match compile("1 + @") {
    Err(e) => println!("{e}"),
    // "unexpected character `@` at offset 4 (line 1, col 5)"
}
```

Error variants:

- `UnexpectedChar` – invalid character during lexing
- `UnterminatedString` – unclosed string literal
- `UnexpectedToken` – parser expected a different token
- `RecursionLimitExceeded` – nesting depth > 64
- `UndefinedVariable` – unknown variable reference
- `TypeMismatch` – incompatible operand types
- `DivisionByZero` – division or modulo by zero
- `IntegerOverflow` – i64 arithmetic overflow
- `IndexOutOfBounds` – list index out of range
- `StepLimitExceeded` – execution budget exhausted

## Running Tests

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```
