//! # expr_eval
//!
//! A secure, deterministic, zero-dependency expression evaluator for business
//! rules and workflows.
//!
//! ## Features
//!
//! - **Typed values**: `null`, `bool`, `int` (i64), `float` (f64), `str`, `list`, `map`
//! - **Safe arithmetic**: overflow-checked integer operations, division-by-zero rejection
//! - **Short-circuit evaluation**: `&&`, `||`, and `? :` never evaluate the unused branch
//! - **Recursion limit**: expressions deeper than 64 levels are rejected at parse time
//! - **Step budget**: configurable execution cap prevents runaway evaluations
//! - **Zero dependencies**: std only, no external crates
//!
//! ## Quick start
//!
//! ```
//! use expr_eval::{compile, EvalContext, Value};
//!
//! let expr = compile("x + 1 > 5 && y contains 'l'").unwrap();
//!
//! let mut ctx = EvalContext::new();
//! ctx.set_variable("x", Value::Int(10));
//! ctx.set_variable("y", Value::Str("hello".into()));
//!
//! let result = expr.eval(&ctx).unwrap();
//! assert_eq!(result, Value::Bool(true));
//! ```
//!
//! ## Operator precedence (lowest to highest)
//!
//! | Level | Operators |
//! |-------|-----------|
//! | 1     | `? :` (ternary) |
//! | 2     | `||` |
//! | 3     | `&&` |
//! | 4     | `==` `!=` |
//! | 5     | `<` `<=` `>` `>=` |
//! | 6     | `contains` `starts_with` `ends_with` `in` |
//! | 7     | `+` `-` |
//! | 8     | `*` `/` `%` |
//! | 9     | unary `!` `-` |
//! | 10    | postfix `.` `[]` `()` |
//!
//! ## Built-in functions
//!
//! | Function | Description |
//! |----------|-------------|
//! | `len(x)` | Length of a string, list, or map |
//! | `lower(s)` | Lowercase a string |
//! | `upper(s)` | Uppercase a string |
//! | `trim(s)` | Trim whitespace from a string |
//! | `abs(n)` | Absolute value of a number |
//! | `min(a, b)` | Minimum of two numbers |
//! | `max(a, b)` | Maximum of two numbers |
//! | `coalesce(a, b, ...)` | First non-null argument |

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod value;

pub use crate::error::{ExprError, MAX_RECURSION_DEPTH};
pub use crate::eval::compile;
pub use crate::parser::Expr;
pub use crate::value::{EvalContext, NativeFn, Value, ValueError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctest_simple() {
        let expr = compile("1 + 2 * 3").unwrap();
        let ctx = EvalContext::new();
        let result = expr.eval(&ctx).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn doctest_variables() {
        let expr = compile("x * 2 + 1").unwrap();
        let mut ctx = EvalContext::new();
        ctx.set_variable("x", Value::Int(5));
        let result = expr.eval(&ctx).unwrap();
        assert_eq!(result, Value::Int(11));
    }

    #[test]
    fn doctest_string_ops() {
        let expr = compile("upper('hello') == 'HELLO'").unwrap();
        let ctx = EvalContext::new();
        let result = expr.eval(&ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }
}
