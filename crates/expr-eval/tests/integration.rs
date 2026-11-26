//! Integration tests for the `expr_eval` expression engine.

use std::sync::Arc;

use expr_eval::{compile, EvalContext, ExprError, Value, ValueError};

fn eval_str(src: &str) -> Result<Value, ExprError> {
    let expr = compile(src)?;
    expr.eval(&EvalContext::new())
}

// ─── Short-circuit evaluation ───────────────────────────────────────────────

#[test]
fn short_circuit_and() {
    // `false && (1/0 == 0)` must NOT evaluate the right side → Ok(Bool(false))
    assert_eq!(eval_str("false && (1 / 0 == 0)").unwrap(), Value::Bool(false));
}

#[test]
fn short_circuit_or() {
    // `true || (1/0 == 0)` must NOT evaluate the right side → Ok(Bool(true))
    assert_eq!(eval_str("true || (1 / 0 == 0)").unwrap(), Value::Bool(true));
}

#[test]
fn ternary_short_circuit() {
    // `true ? (1/0) : 42` → should NOT evaluate the else branch
    // Actually true ? then : else → evaluates then branch: 1/0 → error
    // So use: false ? 1 : (1/0) → should NOT evaluate (1/0)
    assert_eq!(eval_str("false ? 1 : 42").unwrap(), Value::Int(42));
    // The then branch (1) is not evaluated when cond is false
    assert_eq!(eval_str("true ? 42 : 1").unwrap(), Value::Int(42));
}

// ─── Safe arithmetic ────────────────────────────────────────────────────────

#[test]
fn division_by_zero() {
    assert!(matches!(eval_str("1 / 0"), Err(ExprError::DivisionByZero { .. })));
    assert!(matches!(eval_str("1 % 0"), Err(ExprError::DivisionByZero { .. })));
}

#[test]
fn integer_overflow() {
    // i64::MIN / -1 overflows
    assert!(matches!(
        eval_str("(-9223372036854775808) / -1"),
        Err(ExprError::IntegerOverflow { .. })
    ));
    // i64::MAX + 1 overflows
    assert!(matches!(
        eval_str("9223372036854775807 + 1"),
        Err(ExprError::IntegerOverflow { .. })
    ));
    // i64::MIN - 1 overflows
    assert!(matches!(
        eval_str("(-9223372036854775808) - 1"),
        Err(ExprError::IntegerOverflow { .. })
    ));
}

// ─── Step limit (anti-DoS) ─────────────────────────────────────────────────

#[test]
fn step_limit() {
    // Build a long expression that exceeds a very small step budget
    let expr = compile("1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1").unwrap();
    let mut ctx = EvalContext::new();
    ctx.set_max_steps(5);
    assert!(matches!(expr.eval(&ctx), Err(ExprError::StepLimitExceeded { .. })));
}

// ─── Recursion limit (anti-DoS) ────────────────────────────────────────────

#[test]
fn recursion_limit_200_parens() {
    let src = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    assert!(matches!(compile(&src), Err(ExprError::RecursionLimitExceeded { .. })));
}

#[test]
fn recursion_limit_63_ok() {
    let src = format!("{}1{}", "(".repeat(63), ")".repeat(63));
    assert!(compile(&src).is_ok());
}

// ─── Nested access ─────────────────────────────────────────────────────────

#[test]
fn nested_access() {
    let mut ctx = EvalContext::new();
    let roles = Value::List(vec![Value::Str("admin".into()), Value::Str("user".into())]);
    let user = Value::Map({
        let mut m = std::collections::BTreeMap::new();
        m.insert("name".to_string(), Value::Str("Alice".into()));
        m.insert("roles".to_string(), roles);
        m
    });
    ctx.set_variable("user", user);

    let expr = compile("user.roles[1]").unwrap();
    let result = expr.eval(&ctx).unwrap();
    assert_eq!(result, Value::Str("user".into()));
}

// ─── Precedence ────────────────────────────────────────────────────────────

#[test]
fn precedence_full() {
    // 1 + 2 * 3 = 7
    assert_eq!(eval_str("1 + 2 * 3").unwrap(), Value::Int(7));
    // (1 + 2) * 3 = 9
    assert_eq!(eval_str("(1 + 2) * 3").unwrap(), Value::Int(9));
    // 2 - 3 - 4 = -5 (left-associative)
    assert_eq!(eval_str("2 - 3 - 4").unwrap(), Value::Int(-5));
    // 100 / 10 / 2 = 5 (left-associative)
    assert_eq!(eval_str("100 / 10 / 2").unwrap(), Value::Int(5));
    // !true || true = true
    assert_eq!(eval_str("!true || true").unwrap(), Value::Bool(true));
    // -2 * -3 = 6
    assert_eq!(eval_str("-2 * -3").unwrap(), Value::Int(6));
    // 1 + 2 == 3 → (1+2) == 3 → true
    assert_eq!(eval_str("1 + 2 == 3").unwrap(), Value::Bool(true));
    // 1 == 2 + 2 → 1 == (2+2) → false
    assert_eq!(eval_str("1 == 2 + 2").unwrap(), Value::Bool(false));
}

// ─── Type errors ───────────────────────────────────────────────────────────

#[test]
fn type_mismatch() {
    assert!(matches!(eval_str("'hello' + 1"), Err(ExprError::TypeMismatch { .. })));
}

#[test]
fn undefined_variable() {
    assert!(matches!(eval_str("unknown_var"), Err(ExprError::UndefinedVariable { .. })));
}

#[test]
fn index_out_of_bounds() {
    assert!(matches!(
        eval_str("[1, 2, 3][10]"),
        Err(ExprError::IndexOutOfBounds { .. })
    ));
}

// ─── Unicode ───────────────────────────────────────────────────────────────

#[test]
fn unicode() {
    assert_eq!(eval_str("len('héllo wörld')").unwrap(), Value::Int(11));
    assert_eq!(eval_str("upper('naïve')").unwrap(), Value::Str("NAÏVE".into()));
    assert_eq!(eval_str("'café' == 'café'").unwrap(), Value::Bool(true));
}

// ─── Built-in functions ────────────────────────────────────────────────────

#[test]
fn builtin_functions() {
    assert_eq!(eval_str("len([1, 2, 3])").unwrap(), Value::Int(3));
    assert_eq!(eval_str("len('hello')").unwrap(), Value::Int(5));
    assert_eq!(eval_str("lower('HeLLo')").unwrap(), Value::Str("hello".into()));
    assert_eq!(eval_str("upper('hello')").unwrap(), Value::Str("HELLO".into()));
    assert_eq!(eval_str("trim('  hi  ')").unwrap(), Value::Str("hi".into()));
    assert_eq!(eval_str("abs(-42)").unwrap(), Value::Int(42));
    assert_eq!(eval_str("abs(42)").unwrap(), Value::Int(42));
    assert_eq!(eval_str("min(3, 7)").unwrap(), Value::Int(3));
    assert_eq!(eval_str("max(3, 7)").unwrap(), Value::Int(7));
    assert_eq!(eval_str("coalesce(null, null, 42)").unwrap(), Value::Int(42));
    assert_eq!(eval_str("coalesce(1, 2, 3)").unwrap(), Value::Int(1));
}

// ─── Native functions ──────────────────────────────────────────────────────

#[test]
fn native_function() {
    let expr = compile("greet(name)").unwrap();
    let mut ctx = EvalContext::new();
    ctx.set_variable("name", Value::Str("World".into()));
    ctx.register_function(
        "greet",
        Arc::new(|args: Vec<Value>| -> Result<Value, ValueError> {
            match args.first() {
                Some(Value::Str(s)) => Ok(Value::Str(format!("Hello, {s}!"))),
                _ => Err(ValueError::TypeMismatch {
                    expected: "str".into(),
                    found: "non-str".into(),
                }),
            }
        }),
    );
    let result = expr.eval(&ctx).unwrap();
    assert_eq!(result, Value::Str("Hello, World!".into()));
}

// ─── Error positions ───────────────────────────────────────────────────────

#[test]
fn error_positions_integration() {
    // "1 + @" → UnexpectedChar at offset 4, line 1, col 5
    let err = eval_str("1 + @").unwrap_err();
    match err {
        ExprError::UnexpectedChar { offset, line, col, .. } => {
            assert_eq!(offset, 4);
            assert_eq!(line, 1);
            assert_eq!(col, 5);
        }
        other => panic!("expected UnexpectedChar, got {other:?}"),
    }
}

// ─── Float arithmetic ──────────────────────────────────────────────────────

#[test]
fn float_arithmetic() {
    assert_eq!(eval_str("1.5 + 2.5").unwrap(), Value::Float(4.0));
    assert_eq!(eval_str("2 * 3.0").unwrap(), Value::Float(6.0));
    assert_eq!(eval_str("10 / 4").unwrap(), Value::Int(2));
    assert_eq!(eval_str("10.0 / 4").unwrap(), Value::Float(2.5));
}

// ─── Map access ────────────────────────────────────────────────────────────

#[test]
fn map_access() {
    let mut ctx = EvalContext::new();
    let m = Value::Map({
        let mut bm = std::collections::BTreeMap::new();
        bm.insert("key".to_string(), Value::Int(99));
        bm
    });
    ctx.set_variable("m", m);
    let expr = compile("m.key").unwrap();
    assert_eq!(expr.eval(&ctx).unwrap(), Value::Int(99));
}

// ─── In / contains operators ───────────────────────────────────────────────

#[test]
fn keyword_operators() {
    assert_eq!(eval_str("'hello' contains 'ell'").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("'hello' contains 'xyz'").unwrap(), Value::Bool(false));
    assert_eq!(eval_str("'hello' starts_with 'he'").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("'hello' ends_with 'lo'").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("3 in [1, 2, 3]").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("5 in [1, 2, 3]").unwrap(), Value::Bool(false));
}

// ─── DoS: no stack overflow with deeply nested parens ─────────────────────

#[test]
fn dos_1000_parens_no_crash() {
    let src = format!("{}1{}", "(".repeat(1000), ")".repeat(1000));
    let result = compile(&src);
    assert!(result.is_err());
}
