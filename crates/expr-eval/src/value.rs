//! Core value types and evaluation context for `expr_eval`.
//!
//! [`Value`] represents any value the expression engine can produce or consume.
//! [`EvalContext`] carries user-supplied variables, native functions, and the
//! execution step budget.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// A runtime value produced or consumed by the expression engine.
///
/// The type system is deliberately small and closed:
/// - [`Value::Null`]
/// - [`Value::Bool`](bool)
/// - [`Value::Int`](i64)
/// - [`Value::Float`](f64)
/// - [`Value::Str`](String)
/// - [`Value::List`](Vec<Value>)
/// - [`Value::Map`](BTreeMap<String, Value>)
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The null / absent value.
    Null,
    /// A boolean value.
    Bool(bool),
    /// A signed 64-bit integer.
    Int(i64),
    /// A 64-bit floating-point number.
    Float(f64),
    /// A UTF-8 string.
    Str(String),
    /// An ordered list of values.
    List(Vec<Value>),
    /// An ordered map from string keys to values (deterministic iteration).
    Map(BTreeMap<String, Value>),
}

impl Eq for Value {}

impl Value {
    /// Returns the type name as a human-readable string (useful in error messages).
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::List(_) => "list",
            Value::Map(_) => "map",
        }
    }

    /// Attempts to coerce the value to a `bool`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Attempts to coerce the value to an `i64`.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Attempts to coerce the value to an `f64`.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Attempts to borrow the value as a `&str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Attempts to borrow the value as a list slice.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l.as_slice()),
            _ => None,
        }
    }

    /// Attempts to borrow the value as a map reference.
    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Returns `true` if the value is [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Str(s) => write!(f, "\"{s}\""),
            Value::List(l) => {
                let items: Vec<String> = l.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Value::Map(m) => {
                let items: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{{ {} }}", items.join(", "))
            }
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Int(i)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_owned())
    }
}

/// A function callable from within an expression.
///
/// Functions are stored behind an [`Arc`] so that they can be cloned cheaply
/// into multiple contexts without any global state.
pub type NativeFn = Arc<dyn Fn(Vec<Value>) -> Result<Value, ValueError> + Send + Sync>;

/// A lightweight error type used *inside* the native-function boundary.
/// It is immediately converted to [`crate::ExprError::TypeMismatch`] or
/// propagated as-is when it carries enough context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    /// The argument type was wrong.
    TypeMismatch {
        /// Expected type description.
        expected: String,
        /// Found type description.
        found: String,
    },
    /// The argument count was wrong.
    WrongArity {
        /// Expected argument count (or "1..=" style description).
        expected: String,
        /// Actual argument count.
        found: usize,
    },
    /// A domain-specific error (e.g. index out of range).
    Domain {
        /// Human-readable message.
        message: String,
    },
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueError::TypeMismatch { expected, found } => {
                write!(f, "type mismatch: expected {expected}, found {found}")
            }
            ValueError::WrongArity { expected, found } => {
                write!(f, "wrong arity: expected {expected} argument(s), got {found}")
            }
            ValueError::Domain { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ValueError {}

/// The evaluation context passed to every call to [`crate::Expr::eval`].
///
/// It holds:
/// - a set of named **variables** (the expression's free inputs),
/// - a set of **native functions** registered by the host application,
/// - a **step budget** (`max_steps`) that caps total AST nodes visited.
///
/// # Thread-safety
///
/// `EvalContext` is `Send` and `Sync` because all fields are `Send + Sync`.
#[derive(Clone)]
pub struct EvalContext {
    /// User-supplied variables keyed by name.
    pub(crate) variables: BTreeMap<String, Value>,
    /// Native functions registered by the host.
    pub(crate) functions: BTreeMap<String, NativeFn>,
    /// Maximum number of AST nodes to evaluate (default: 10 000).
    pub(crate) max_steps: usize,
}

impl fmt::Debug for EvalContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvalContext")
            .field("variables", &self.variables)
            .field("functions", &format!("{} registered", self.functions.len()))
            .field("max_steps", &self.max_steps)
            .finish()
    }
}

impl Default for EvalContext {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalContext {
    /// Creates an empty context with the default step budget (10 000 steps).
    pub fn new() -> Self {
        Self {
            variables: BTreeMap::new(),
            functions: BTreeMap::new(),
            max_steps: 10_000,
        }
    }

    /// Sets a variable by name.
    ///
    /// # Examples
    /// ```
    /// use expr_eval::{EvalContext, Value};
    /// let mut ctx = EvalContext::new();
    /// ctx.set_variable("x", Value::Int(42));
    /// ```
    pub fn set_variable(&mut self, name: impl Into<String>, value: Value) -> &mut Self {
        self.variables.insert(name.into(), value);
        self
    }

    /// Returns the value of a variable, if present.
    pub fn get_variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    /// Registers a native function callable from expressions.
    ///
    /// The function receives the argument list and returns a [`Value`] or a
    /// [`ValueError`].
    pub fn register_function(
        &mut self,
        name: impl Into<String>,
        func: Arc<dyn Fn(Vec<Value>) -> Result<Value, ValueError> + Send + Sync>,
    ) -> &mut Self {
        self.functions.insert(name.into(), func);
        self
    }

    /// Returns the registered function, if any.
    pub fn get_function(&self, name: &str) -> Option<&NativeFn> {
        self.functions.get(name)
    }

    /// Sets the maximum number of evaluation steps (default 10 000).
    pub fn set_max_steps(&mut self, max_steps: usize) -> &mut Self {
        self.max_steps = max_steps;
        self
    }

    /// Returns the configured step budget.
    pub fn max_steps(&self) -> usize {
        self.max_steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_type_names() {
        assert_eq!(Value::Null.type_name(), "null");
        assert_eq!(Value::Bool(true).type_name(), "bool");
        assert_eq!(Value::Int(1).type_name(), "int");
        assert_eq!(Value::Float(1.0).type_name(), "float");
        assert_eq!(Value::Str("a".into()).type_name(), "str");
        assert_eq!(Value::List(vec![]).type_name(), "list");
        assert_eq!(Value::Map(BTreeMap::new()).type_name(), "map");
    }

    #[test]
    fn value_display() {
        assert_eq!(Value::Null.to_string(), "null");
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::Int(42).to_string(), "42");
        assert_eq!(Value::Float(1.5).to_string(), "1.5");
        assert_eq!(Value::Str("hi".into()).to_string(), "\"hi\"");
        assert_eq!(Value::List(vec![Value::Int(1), Value::Int(2)]).to_string(), "[1, 2]");
    }

    #[test]
    fn context_set_get_variable() {
        let mut ctx = EvalContext::new();
        ctx.set_variable("x", Value::Int(10));
        assert_eq!(ctx.get_variable("x"), Some(&Value::Int(10)));
        assert_eq!(ctx.get_variable("y"), None);
    }

    #[test]
    fn context_default_steps() {
        let ctx = EvalContext::new();
        assert_eq!(ctx.max_steps(), 10_000);
    }
}