//! Evaluation engine: `compile()` and `Expr::eval()`.
//!
//! The evaluator is deterministic, short-circuits `&&`, `||` and `? :`, uses
//! checked integer arithmetic, and enforces a configurable step budget.

use std::collections::BTreeMap;

use crate::error::ExprError;
use crate::lexer::Lexer;
use crate::parser::{BinOp, Expr, Parser, UnOp};
use crate::value::{EvalContext, Value, ValueError};

/// Compiles a source expression into an executable [`Expr`] AST.
///
/// # Errors
///
/// Returns an [`ExprError`] if the source fails to lex or parse, or if the
/// nesting depth exceeds [`crate::MAX_RECURSION_DEPTH`].
///
/// # Examples
///
/// ```
/// use expr_eval::compile;
///
/// let expr = compile("1 + 2 * 3").unwrap();
/// assert!(format!("{expr:?}").contains("Binary"));
/// ```
pub fn compile(src: &str) -> Result<Expr, ExprError> {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}

/// Internal evaluation state: tracks the remaining step budget.
struct Evaluator<'a> {
    ctx: &'a EvalContext,
    steps: usize,
}

impl<'a> Evaluator<'a> {
    fn new(ctx: &'a EvalContext) -> Self {
        Evaluator {
            ctx,
            steps: 0,
        }
    }

    fn tick(&mut self) -> Result<(), ExprError> {
        self.steps += 1;
        if self.steps > self.ctx.max_steps() {
            return Err(ExprError::StepLimitExceeded {
                max_steps: self.ctx.max_steps(),
            });
        }
        Ok(())
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, ExprError> {
        self.tick()?;
        match expr {
            Expr::Null => Ok(Value::Null),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.eval(item)?);
                }
                Ok(Value::List(out))
            }
            Expr::Map(pairs) => {
                let mut out = BTreeMap::new();
                for (k, v) in pairs {
                    out.insert(k.clone(), self.eval(v)?);
                }
                Ok(Value::Map(out))
            }
            Expr::Var(name) => match self.ctx.get_variable(name) {
                Some(v) => Ok(v.clone()),
                None => Err(ExprError::UndefinedVariable {
                    name: name.clone(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
            Expr::Unary { op, operand } => {
                let v = self.eval(operand)?;
                self.eval_unary(*op, v)
            }
            Expr::Binary { op, lhs, rhs } => self.eval_binary(*op, lhs, rhs),
            Expr::Ternary { cond, then, els } => {
                let c = self.eval(cond)?;
                match c.as_bool() {
                    Some(true) => self.eval(then),
                    Some(false) => self.eval(els),
                    None => Err(ExprError::TypeMismatch {
                        expected: "bool".into(),
                        found: c.type_name().into(),
                        offset: 0,
                        line: 1,
                        col: 1,
                    }),
                }
            }
            Expr::Call { name, args } => self.eval_call(name, args),
            Expr::Member { obj, field } => {
                let base = self.eval(obj)?;
                match base {
                    Value::Map(m) => match m.get(field) {
                        Some(v) => Ok(v.clone()),
                        None => Ok(Value::Null),
                    },
                    other => Err(ExprError::TypeMismatch {
                        expected: "map".into(),
                        found: other.type_name().into(),
                        offset: 0,
                        line: 1,
                        col: 1,
                    }),
                }
            }
            Expr::Index { obj, index } => {
                let base = self.eval(obj)?;
                let idx = self.eval(index)?;
                self.eval_index(base, idx)
            }
        }
    }

    fn eval_unary(&self, op: UnOp, v: Value) -> Result<Value, ExprError> {
        match op {
            UnOp::Not => match v {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                other => Err(ExprError::TypeMismatch {
                    expected: "bool".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
            UnOp::Neg => match v {
                Value::Int(i) => i.checked_neg().map(Value::Int).ok_or(ExprError::IntegerOverflow {
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
                Value::Float(f) => Ok(Value::Float(-f)),
                other => Err(ExprError::TypeMismatch {
                    expected: "number".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
        }
    }

    fn eval_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, ExprError> {
        // Short-circuit operators first.
        match op {
            BinOp::And => {
                let l = self.eval(lhs)?;
                let lb = l.as_bool().ok_or_else(|| ExprError::TypeMismatch {
                    expected: "bool".into(),
                    found: l.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                })?;
                if !lb {
                    return Ok(Value::Bool(false));
                }
                let r = self.eval(rhs)?;
                let rb = r.as_bool().ok_or_else(|| ExprError::TypeMismatch {
                    expected: "bool".into(),
                    found: r.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                })?;
                return Ok(Value::Bool(rb));
            }
            BinOp::Or => {
                let l = self.eval(lhs)?;
                let lb = l.as_bool().ok_or_else(|| ExprError::TypeMismatch {
                    expected: "bool".into(),
                    found: l.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                })?;
                if lb {
                    return Ok(Value::Bool(true));
                }
                let r = self.eval(rhs)?;
                let rb = r.as_bool().ok_or_else(|| ExprError::TypeMismatch {
                    expected: "bool".into(),
                    found: r.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                })?;
                return Ok(Value::Bool(rb));
            }
            _ => {}
        }

        let l = self.eval(lhs)?;
        let r = self.eval(rhs)?;
        self.apply_binary(op, l, r)
    }

    fn apply_binary(&self, op: BinOp, l: Value, r: Value) -> Result<Value, ExprError> {
        match op {
            BinOp::Add => self.arith_add(l, r),
            BinOp::Sub => self.arith_sub(l, r),
            BinOp::Mul => self.arith_mul(l, r),
            BinOp::Div => self.arith_div(l, r),
            BinOp::Rem => self.arith_rem(l, r),
            BinOp::Eq => Ok(Value::Bool(values_equal(&l, &r))),
            BinOp::NotEq => Ok(Value::Bool(!values_equal(&l, &r))),
            BinOp::Lt => self.compare(l, r, |o| o == std::cmp::Ordering::Less),
            BinOp::LtEq => self.compare(l, r, |o| o != std::cmp::Ordering::Greater),
            BinOp::Gt => self.compare(l, r, |o| o == std::cmp::Ordering::Greater),
            BinOp::GtEq => self.compare(l, r, |o| o != std::cmp::Ordering::Less),
            BinOp::Contains => self.str_op(l, r, |a, b| a.contains(b)),
            BinOp::StartsWith => self.str_op(l, r, |a, b| a.starts_with(b)),
            BinOp::EndsWith => self.str_op(l, r, |a, b| a.ends_with(b)),
            BinOp::In => self.op_in(l, r),
            BinOp::And | BinOp::Or => unreachable!("handled in eval_binary"),
        }
    }

    fn arith_add(&self, l: Value, r: Value) -> Result<Value, ExprError> {
        match (l, r) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_add(b)
                .map(Value::Int)
                .ok_or(ExprError::IntegerOverflow {
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(a + &b)),
            (a, b) => Err(ExprError::TypeMismatch {
                expected: "number or string".into(),
                found: format!("{} + {}", a.type_name(), b.type_name()),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn arith_sub(&self, l: Value, r: Value) -> Result<Value, ExprError> {
        match (l, r) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_sub(b)
                .map(Value::Int)
                .ok_or(ExprError::IntegerOverflow {
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - b as f64)),
            (a, b) => Err(ExprError::TypeMismatch {
                expected: "number".into(),
                found: format!("{} - {}", a.type_name(), b.type_name()),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn arith_mul(&self, l: Value, r: Value) -> Result<Value, ExprError> {
        match (l, r) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_mul(b)
                .map(Value::Int)
                .ok_or(ExprError::IntegerOverflow {
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
            (a, b) => Err(ExprError::TypeMismatch {
                expected: "number".into(),
                found: format!("{} * {}", a.type_name(), b.type_name()),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn arith_div(&self, l: Value, r: Value) -> Result<Value, ExprError> {
        match (l, r) {
            (Value::Int(_), Value::Int(0)) => Err(ExprError::DivisionByZero {
                offset: 0,
                line: 1,
                col: 1,
            }),
            (Value::Int(a), Value::Int(b)) => {
                a.checked_div(b).map(Value::Int).ok_or(ExprError::IntegerOverflow {
                    offset: 0,
                    line: 1,
                    col: 1,
                })
            }
            (Value::Float(a), Value::Float(b)) => {
                if b == 0.0 {
                    Err(ExprError::DivisionByZero {
                        offset: 0,
                        line: 1,
                        col: 1,
                    })
                } else {
                    Ok(Value::Float(a / b))
                }
            }
            (Value::Int(a), Value::Float(b)) => {
                if b == 0.0 {
                    Err(ExprError::DivisionByZero {
                        offset: 0,
                        line: 1,
                        col: 1,
                    })
                } else {
                    Ok(Value::Float(a as f64 / b))
                }
            }
            (Value::Float(a), Value::Int(b)) => {
                if b == 0 {
                    Err(ExprError::DivisionByZero {
                        offset: 0,
                        line: 1,
                        col: 1,
                    })
                } else {
                    Ok(Value::Float(a / b as f64))
                }
            }
            (a, b) => Err(ExprError::TypeMismatch {
                expected: "number".into(),
                found: format!("{} / {}", a.type_name(), b.type_name()),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn arith_rem(&self, l: Value, r: Value) -> Result<Value, ExprError> {
        match (l, r) {
            (Value::Int(_), Value::Int(0)) => Err(ExprError::DivisionByZero {
                offset: 0,
                line: 1,
                col: 1,
            }),
            (Value::Int(a), Value::Int(b)) => {
                a.checked_rem(b).map(Value::Int).ok_or(ExprError::IntegerOverflow {
                    offset: 0,
                    line: 1,
                    col: 1,
                })
            }
            (a, b) => Err(ExprError::TypeMismatch {
                expected: "int".into(),
                found: format!("{} % {}", a.type_name(), b.type_name()),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn compare<F>(&self, l: Value, r: Value, pred: F) -> Result<Value, ExprError>
    where
        F: Fn(std::cmp::Ordering) -> bool,
    {
        let ord = match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Float(a), Value::Float(b)) => a
                .partial_cmp(b)
                .ok_or_else(|| ExprError::TypeMismatch {
                    expected: "comparable numbers".into(),
                    found: "NaN".into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                })?,
            (Value::Int(a), Value::Float(b)) => (*a as f64)
                .partial_cmp(b)
                .ok_or_else(|| ExprError::TypeMismatch {
                    expected: "comparable numbers".into(),
                    found: "NaN".into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                })?,
            (Value::Float(a), Value::Int(b)) => a
                .partial_cmp(&(*b as f64))
                .ok_or_else(|| ExprError::TypeMismatch {
                    expected: "comparable numbers".into(),
                    found: "NaN".into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                })?,
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            (a, b) => {
                return Err(ExprError::TypeMismatch {
                    expected: "comparable values".into(),
                    found: format!("{} vs {}", a.type_name(), b.type_name()),
                    offset: 0,
                    line: 1,
                    col: 1,
                })
            }
        };
        Ok(Value::Bool(pred(ord)))
    }

    fn str_op<F>(&self, l: Value, r: Value, pred: F) -> Result<Value, ExprError>
    where
        F: Fn(&str, &str) -> bool,
    {
        match (l, r) {
            (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(pred(&a, &b))),
            (a, b) => Err(ExprError::TypeMismatch {
                expected: "str".into(),
                found: format!("{} vs {}", a.type_name(), b.type_name()),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn op_in(&self, l: Value, r: Value) -> Result<Value, ExprError> {
        match r {
            Value::List(items) => Ok(Value::Bool(items.iter().any(|v| values_equal(v, &l)))),
            Value::Map(m) => match l {
                Value::Str(s) => Ok(Value::Bool(m.contains_key(&s))),
                other => Err(ExprError::TypeMismatch {
                    expected: "str".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
            Value::Str(hay) => match l {
                Value::Str(needle) => Ok(Value::Bool(hay.contains(&needle))),
                other => Err(ExprError::TypeMismatch {
                    expected: "str".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
            other => Err(ExprError::TypeMismatch {
                expected: "list, map or str".into(),
                found: other.type_name().into(),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn eval_index(&self, base: Value, idx: Value) -> Result<Value, ExprError> {
        match base {
            Value::List(items) => match idx {
                Value::Int(i) => {
                    if i < 0 || (i as usize) >= items.len() {
                        Err(ExprError::IndexOutOfBounds {
                            offset: 0,
                            line: 1,
                            col: 1,
                        })
                    } else {
                        Ok(items[i as usize].clone())
                    }
                }
                other => Err(ExprError::TypeMismatch {
                    expected: "int".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
            Value::Map(m) => match idx {
                Value::Str(s) => Ok(m.get(&s).cloned().unwrap_or(Value::Null)),
                other => Err(ExprError::TypeMismatch {
                    expected: "str".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
            Value::Str(s) => match idx {
                Value::Int(i) => {
                    let chars: Vec<char> = s.chars().collect();
                    if i < 0 || (i as usize) >= chars.len() {
                        Err(ExprError::IndexOutOfBounds {
                            offset: 0,
                            line: 1,
                            col: 1,
                        })
                    } else {
                        Ok(Value::Str(chars[i as usize].to_string()))
                    }
                }
                other => Err(ExprError::TypeMismatch {
                    expected: "int".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            },
            other => Err(ExprError::TypeMismatch {
                expected: "list, map or str".into(),
                found: other.type_name().into(),
                offset: 0,
                line: 1,
                col: 1,
            }),
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<Value, ExprError> {
        let mut values = Vec::with_capacity(args.len());
        for a in args {
            values.push(self.eval(a)?);
        }
        // User-registered native functions take precedence.
        if let Some(f) = self.ctx.get_function(name) {
            return f(values).map_err(|e| match e {
                ValueError::TypeMismatch { expected, found } => ExprError::TypeMismatch {
                    expected,
                    found,
                    offset: 0,
                    line: 1,
                    col: 1,
                },
                ValueError::WrongArity { expected, found } => ExprError::TypeMismatch {
                    expected: expected.clone(),
                    found: format!("{found} argument(s) (expected {expected})"),
                    offset: 0,
                    line: 1,
                    col: 1,
                },
                ValueError::Domain { message } => ExprError::TypeMismatch {
                    expected: "valid argument".into(),
                    found: message,
                    offset: 0,
                    line: 1,
                    col: 1,
                },
            });
        }
        builtin_call(name, values)
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
        (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
        _ => a == b,
    }
}

fn builtin_call(name: &str, args: Vec<Value>) -> Result<Value, ExprError> {
    match name {
        "len" => {
            expect_arity(name, &args, 1)?;
            match &args[0] {
                Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                Value::List(l) => Ok(Value::Int(l.len() as i64)),
                Value::Map(m) => Ok(Value::Int(m.len() as i64)),
                other => Err(ExprError::TypeMismatch {
                    expected: "str, list or map".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            }
        }
        "lower" => {
            expect_arity(name, &args, 1)?;
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.to_lowercase())),
                other => Err(ExprError::TypeMismatch {
                    expected: "str".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            }
        }
        "upper" => {
            expect_arity(name, &args, 1)?;
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.to_uppercase())),
                other => Err(ExprError::TypeMismatch {
                    expected: "str".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            }
        }
        "trim" => {
            expect_arity(name, &args, 1)?;
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.trim().to_owned())),
                other => Err(ExprError::TypeMismatch {
                    expected: "str".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            }
        }
        "abs" => {
            expect_arity(name, &args, 1)?;
            match &args[0] {
                Value::Int(i) => i
                    .checked_abs()
                    .map(Value::Int)
                    .ok_or(ExprError::IntegerOverflow {
                        offset: 0,
                        line: 1,
                        col: 1,
                    }),
                Value::Float(f) => Ok(Value::Float(f.abs())),
                other => Err(ExprError::TypeMismatch {
                    expected: "number".into(),
                    found: other.type_name().into(),
                    offset: 0,
                    line: 1,
                    col: 1,
                }),
            }
        }
        "min" | "max" => {
            expect_arity(name, &args, 2)?;
            let a = &args[0];
            let b = &args[1];
            let ord = match (a, b) {
                (Value::Int(x), Value::Int(y)) => x.cmp(y),
                (Value::Float(x), Value::Float(y)) => {
                    x.partial_cmp(y).ok_or(ExprError::TypeMismatch {
                        expected: "comparable numbers".into(),
                        found: "NaN".into(),
                        offset: 0,
                        line: 1,
                        col: 1,
                    })?
                }
                (Value::Int(x), Value::Float(y)) => (*x as f64)
                    .partial_cmp(y)
                    .ok_or(ExprError::TypeMismatch {
                        expected: "comparable numbers".into(),
                        found: "NaN".into(),
                        offset: 0,
                        line: 1,
                        col: 1,
                    })?,
                (Value::Float(x), Value::Int(y)) => x
                    .partial_cmp(&(*y as f64))
                    .ok_or(ExprError::TypeMismatch {
                        expected: "comparable numbers".into(),
                        found: "NaN".into(),
                        offset: 0,
                        line: 1,
                        col: 1,
                    })?,
                (x, y) => {
                    return Err(ExprError::TypeMismatch {
                        expected: "number".into(),
                        found: format!("{} vs {}", x.type_name(), y.type_name()),
                        offset: 0,
                        line: 1,
                        col: 1,
                    })
                }
            };
            let pick_a = if name == "min" {
                ord != std::cmp::Ordering::Greater
            } else {
                ord != std::cmp::Ordering::Less
            };
            Ok(if pick_a { a.clone() } else { b.clone() })
        }
        "coalesce" => {
            for v in args {
                if !v.is_null() {
                    return Ok(v);
                }
            }
            Ok(Value::Null)
        }
        other => Err(ExprError::UndefinedVariable {
            name: format!("{other}()"),
            offset: 0,
            line: 1,
            col: 1,
        }),
    }
}

fn expect_arity(name: &str, args: &[Value], expected: usize) -> Result<(), ExprError> {
    if args.len() != expected {
        return Err(ExprError::TypeMismatch {
            expected: format!("{expected} argument(s) to `{name}`"),
            found: format!("{} argument(s)", args.len()),
            offset: 0,
            line: 1,
            col: 1,
        });
    }
    Ok(())
}

impl Expr {
    /// Evaluates the compiled expression against the given context.
    ///
    /// # Errors
    ///
    /// Returns an [`ExprError`] for undefined variables, type mismatches,
    /// division by zero, integer overflow, out-of-bounds access, or when the
    /// step budget is exhausted.
    ///
    /// # Examples
    ///
    /// ```
    /// use expr_eval::{compile, EvalContext, Value};
    ///
    /// let expr = compile("1 + 2").unwrap();
    /// let ctx = EvalContext::new();
    /// assert_eq!(expr.eval(&ctx).unwrap(), Value::Int(3));
    /// ```
    pub fn eval(&self, ctx: &EvalContext) -> Result<Value, ExprError> {
        let mut ev = Evaluator::new(ctx);
        ev.eval(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_str(src: &str) -> Result<Value, ExprError> {
        let expr = compile(src)?;
        let ctx = EvalContext::new();
        expr.eval(&ctx)
    }

    #[test]
    fn short_circuit_and() {
        // false && (1/0 == 0) must NOT evaluate the right side.
        let r = eval_str("false && (1 / 0 == 0)").unwrap();
        assert_eq!(r, Value::Bool(false));
    }

    #[test]
    fn short_circuit_or() {
        let r = eval_str("true || (1 / 0 == 0)").unwrap();
        assert_eq!(r, Value::Bool(true));
    }

    #[test]
    fn ternary_short_circuit() {
        let r = eval_str("true ? 1 : (1 / 0)").unwrap();
        assert_eq!(r, Value::Int(1));
        let r2 = eval_str("false ? (1 / 0) : 2").unwrap();
        assert_eq!(r2, Value::Int(2));
    }

    #[test]
    fn division_by_zero() {
        let r = eval_str("1 / 0");
        assert!(matches!(r, Err(ExprError::DivisionByZero { .. })));
        let r2 = eval_str("1 % 0");
        assert!(matches!(r2, Err(ExprError::DivisionByZero { .. })));
    }

    #[test]
    fn integer_overflow() {
        let r = eval_str("9223372036854775807 + 1");
        assert!(matches!(r, Err(ExprError::IntegerOverflow { .. })));
        let r2 = eval_str("-9223372036854775808 / -1");
        assert!(matches!(r2, Err(ExprError::IntegerOverflow { .. })));
    }

    #[test]
    fn step_limit() {
        let expr = compile("1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1").unwrap();
        let mut ctx = EvalContext::new();
        ctx.set_max_steps(2);
        let r = expr.eval(&ctx);
        assert!(matches!(r, Err(ExprError::StepLimitExceeded { .. })));
    }

    #[test]
    fn nested_access() {
        let mut roles = BTreeMap::new();
        roles.insert("admin".to_string(), Value::Bool(true));
        let mut user = BTreeMap::new();
        user.insert(
            "roles".to_string(),
            Value::List(vec![Value::Str("guest".into()), Value::Str("admin".into())]),
        );
        user.insert("meta".to_string(), Value::Map(roles));

        let mut ctx = EvalContext::new();
        ctx.set_variable("user", Value::Map(user));

        let expr = compile("user.roles[1]").unwrap();
        assert_eq!(expr.eval(&ctx).unwrap(), Value::Str("admin".into()));

        let expr2 = compile("user.meta.admin").unwrap();
        assert_eq!(expr2.eval(&ctx).unwrap(), Value::Bool(true));
    }

    #[test]
    fn builtin_functions() {
        assert_eq!(eval_str("len('hello')").unwrap(), Value::Int(5));
        assert_eq!(eval_str("lower('ABC')").unwrap(), Value::Str("abc".into()));
        assert_eq!(eval_str("upper('abc')").unwrap(), Value::Str("ABC".into()));
        assert_eq!(eval_str("trim('  x  ')").unwrap(), Value::Str("x".into()));
        assert_eq!(eval_str("abs(-5)").unwrap(), Value::Int(5));
        assert_eq!(eval_str("min(3, 7)").unwrap(), Value::Int(3));
        assert_eq!(eval_str("max(3, 7)").unwrap(), Value::Int(7));
        assert_eq!(eval_str("coalesce(null, null, 42)").unwrap(), Value::Int(42));
    }

    #[test]
    fn undefined_variable() {
        let r = eval_str("missing");
        assert!(matches!(r, Err(ExprError::UndefinedVariable { .. })));
    }

    #[test]
    fn type_mismatch() {
        let r = eval_str("1 + 'a'");
        assert!(matches!(r, Err(ExprError::TypeMismatch { .. })));
    }

    #[test]
    fn index_out_of_bounds() {
        let r = eval_str("[1, 2][5]");
        assert!(matches!(r, Err(ExprError::IndexOutOfBounds { .. })));
    }

    #[test]
    fn string_ops() {
        assert_eq!(eval_str("'hello' contains 'ell'").unwrap(), Value::Bool(true));
        assert_eq!(eval_str("'hello' starts_with 'he'").unwrap(), Value::Bool(true));
        assert_eq!(eval_str("'hello' ends_with 'lo'").unwrap(), Value::Bool(true));
        assert_eq!(eval_str("'a' in ['a', 'b']").unwrap(), Value::Bool(true));
    }

    #[test]
    fn unicode_strings() {
        assert_eq!(eval_str("len('héllo')").unwrap(), Value::Int(5));
        assert_eq!(eval_str("upper('héllo')").unwrap(), Value::Str("HÉLLO".into()));
    }
}
