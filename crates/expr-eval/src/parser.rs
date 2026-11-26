//! Pratt parser producing a typed AST with a hard recursion limit of 64.
//!
//! Precedence (lowest to highest):
//! 1. `||`
//! 2. `&&`
//! 3. `==` `!=`
//! 4. `<` `<=` `>` `>=`
//! 5. `contains` `starts_with` `ends_with` `in`
//! 6. `+` `-`
//! 7. `*` `/` `%`
//! 8. unary `!` `-`
//! 9. postfix `.` `[]` `()`
//! 10. ternary `? :`

use crate::error::{ExprError, MAX_RECURSION_DEPTH};
use crate::lexer::{Located, Token};

/// A compiled expression (AST node).
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A null literal.
    Null,
    /// A boolean literal.
    Bool(bool),
    /// An integer literal.
    Int(i64),
    /// A float literal.
    Float(f64),
    /// A string literal.
    Str(String),
    /// A list literal.
    List(Vec<Expr>),
    /// A map literal (preserves insertion order via Vec of pairs).
    Map(Vec<(String, Expr)>),
    /// A variable reference.
    Var(String),
    /// A binary operation.
    Binary {
        /// The operator.
        op: BinOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A unary operation.
    Unary {
        /// The operator.
        op: UnOp,
        /// Operand.
        operand: Box<Expr>,
    },
    /// A function call.
    Call {
        /// Function name.
        name: String,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// A member access (dot notation).
    Member {
        /// Base expression.
        obj: Box<Expr>,
        /// Field name.
        field: String,
    },
    /// An index access (bracket notation).
    Index {
        /// Base expression.
        obj: Box<Expr>,
        /// Index expression.
        index: Box<Expr>,
    },
    /// A ternary conditional expression.
    Ternary {
        /// Condition.
        cond: Box<Expr>,
        /// Then branch.
        then: Box<Expr>,
        /// Else branch.
        els: Box<Expr>,
    },
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `==`
    Eq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    LtEq,
    /// `>`
    Gt,
    /// `>=`
    GtEq,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `contains`
    Contains,
    /// `starts_with`
    StartsWith,
    /// `ends_with`
    EndsWith,
    /// `in`
    In,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `!`
    Not,
    /// `-`
    Neg,
}

/// The parser state.
pub struct Parser {
    tokens: Vec<Located>,
    pos: usize,
    depth: usize,
}

impl Parser {
    /// Creates a new parser over the given token stream.
    pub fn new(tokens: Vec<Located>) -> Self {
        Parser {
            tokens,
            pos: 0,
            depth: 0,
        }
    }

    /// Parses the whole token stream into a single expression.
    pub fn parse(&mut self) -> Result<Expr, ExprError> {
        let expr = self.parse_expr(0)?;
        let tok = self.peek();
        if !matches!(tok.token, Token::Eof) {
            return Err(ExprError::UnexpectedToken {
                expected: "end of input".into(),
                found: tok.token.description().into(),
                offset: tok.offset,
                line: tok.line,
                col: tok.col,
            });
        }
        Ok(expr)
    }

    fn peek(&self) -> &Located {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Located {
        let t = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn enter(&mut self, tok: &Located) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_RECURSION_DEPTH {
            return Err(ExprError::RecursionLimitExceeded {
                depth: self.depth,
                offset: tok.offset,
                line: tok.line,
                col: tok.col,
            });
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Parses an expression with the given minimum binding power.
    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, ExprError> {
        let start = self.peek().clone();
        self.enter(&start)?;
        let result = self.parse_expr_inner(min_bp);
        self.leave();
        result
    }

    fn parse_expr_inner(&mut self, min_bp: u8) -> Result<Expr, ExprError> {
        let mut lhs = self.parse_prefix()?;

        loop {
            let tok = self.peek().clone();
            // Ternary has the lowest precedence; handle it specially.
            if matches!(tok.token, Token::Question) && min_bp == 0 {
                self.advance();
                let then = self.parse_expr(0)?;
                let colon = self.peek().clone();
                if !matches!(colon.token, Token::Colon) {
                    return Err(ExprError::UnexpectedToken {
                        expected: "`:`".into(),
                        found: colon.token.description().into(),
                        offset: colon.offset,
                        line: colon.line,
                        col: colon.col,
                    });
                }
                self.advance();
                let els = self.parse_expr(0)?;
                lhs = Expr::Ternary {
                    cond: Box::new(lhs),
                    then: Box::new(then),
                    els: Box::new(els),
                };
                continue;
            }

            let (op, lbp, rbp) = match self.infix_binding(&tok.token) {
                Some(v) => v,
                None => break,
            };
            if lbp < min_bp {
                break;
            }
            self.advance();
            let rhs = self.parse_expr(rbp)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    fn infix_binding(&self, tok: &Token) -> Option<(BinOp, u8, u8)> {
        // (op, left binding power, right binding power)
        let v = match tok {
            Token::Or => (BinOp::Or, 1, 2),
            Token::And => (BinOp::And, 3, 4),
            Token::EqEq => (BinOp::Eq, 5, 6),
            Token::NotEq => (BinOp::NotEq, 5, 6),
            Token::Lt => (BinOp::Lt, 7, 8),
            Token::LtEq => (BinOp::LtEq, 7, 8),
            Token::Gt => (BinOp::Gt, 7, 8),
            Token::GtEq => (BinOp::GtEq, 7, 8),
            Token::Contains => (BinOp::Contains, 9, 10),
            Token::StartsWith => (BinOp::StartsWith, 9, 10),
            Token::EndsWith => (BinOp::EndsWith, 9, 10),
            Token::In => (BinOp::In, 9, 10),
            Token::Plus => (BinOp::Add, 11, 12),
            Token::Minus => (BinOp::Sub, 11, 12),
            Token::Star => (BinOp::Mul, 13, 14),
            Token::Slash => (BinOp::Div, 13, 14),
            Token::Percent => (BinOp::Rem, 13, 14),
            _ => return None,
        };
        Some(v)
    }

    fn parse_prefix(&mut self) -> Result<Expr, ExprError> {
        let tok = self.peek().clone();
        let mut expr = match &tok.token {
            Token::Int(i) => {
                self.advance();
                Expr::Int(*i)
            }
            Token::Float(f) => {
                self.advance();
                Expr::Float(*f)
            }
            Token::Str(s) => {
                self.advance();
                Expr::Str(s.clone())
            }
            Token::BoolTrue => {
                self.advance();
                Expr::Bool(true)
            }
            Token::BoolFalse => {
                self.advance();
                Expr::Bool(false)
            }
            Token::Null => {
                self.advance();
                Expr::Null
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.advance();
                if matches!(self.peek().token, Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek().token, Token::RParen) {
                        loop {
                            args.push(self.parse_expr(0)?);
                            if matches!(self.peek().token, Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    let close = self.peek().clone();
                    if !matches!(close.token, Token::RParen) {
                        return Err(ExprError::UnexpectedToken {
                            expected: "`)`".into(),
                            found: close.token.description().into(),
                            offset: close.offset,
                            line: close.line,
                            col: close.col,
                        });
                    }
                    self.advance();
                    Expr::Call { name, args }
                } else {
                    Expr::Var(name)
                }
            }
            Token::LParen => {
                self.advance();
                let inner = self.parse_expr(0)?;
                let close = self.peek().clone();
                if !matches!(close.token, Token::RParen) {
                    return Err(ExprError::UnexpectedToken {
                        expected: "`)`".into(),
                        found: close.token.description().into(),
                        offset: close.offset,
                        line: close.line,
                        col: close.col,
                    });
                }
                self.advance();
                inner
            }
            Token::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !matches!(self.peek().token, Token::RBracket) {
                    loop {
                        items.push(self.parse_expr(0)?);
                        if matches!(self.peek().token, Token::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                let close = self.peek().clone();
                if !matches!(close.token, Token::RBracket) {
                    return Err(ExprError::UnexpectedToken {
                        expected: "`]`".into(),
                        found: close.token.description().into(),
                        offset: close.offset,
                        line: close.line,
                        col: close.col,
                    });
                }
                self.advance();
                Expr::List(items)
            }
            Token::Not => {
                self.advance();
                let operand = self.parse_expr(15)?;
                Expr::Unary {
                    op: UnOp::Not,
                    operand: Box::new(operand),
                }
            }
            Token::Minus => {
                self.advance();
                let operand = self.parse_expr(15)?;
                Expr::Unary {
                    op: UnOp::Neg,
                    operand: Box::new(operand),
                }
            }
            _ => {
                return Err(ExprError::UnexpectedToken {
                    expected: "expression".into(),
                    found: tok.token.description().into(),
                    offset: tok.offset,
                    line: tok.line,
                    col: tok.col,
                });
            }
        };

        // Postfix: `.field`, `[index]`
        loop {
            let tok = self.peek().clone();
            match tok.token {
                Token::Dot => {
                    self.advance();
                    let field_tok = self.peek().clone();
                    let field = match &field_tok.token {
                        Token::Ident(n) => n.clone(),
                        _ => {
                            return Err(ExprError::UnexpectedToken {
                                expected: "field name".into(),
                                found: field_tok.token.description().into(),
                                offset: field_tok.offset,
                                line: field_tok.line,
                                col: field_tok.col,
                            });
                        }
                    };
                    self.advance();
                    expr = Expr::Member {
                        obj: Box::new(expr),
                        field,
                    };
                }
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr(0)?;
                    let close = self.peek().clone();
                    if !matches!(close.token, Token::RBracket) {
                        return Err(ExprError::UnexpectedToken {
                            expected: "`]`".into(),
                            found: close.token.description().into(),
                            offset: close.offset,
                            line: close.line,
                            col: close.col,
                        });
                    }
                    self.advance();
                    expr = Expr::Index {
                        obj: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Result<Expr, ExprError> {
        let mut lx = Lexer::new(src);
        let tokens = lx.tokenize()?;
        let mut p = Parser::new(tokens);
        p.parse()
    }

    #[test]
    fn precedence() {
        // 1 + 2 * 3 == 7  →  (1 + (2*3)) == 7
        let e = parse("1 + 2 * 3 == 7").unwrap();
        match e {
            Expr::Binary { op: BinOp::Eq, lhs, .. } => match *lhs {
                Expr::Binary { op: BinOp::Add, rhs, .. } => {
                    assert!(matches!(*rhs, Expr::Binary { op: BinOp::Mul, .. }));
                }
                other => panic!("expected Add, got {other:?}"),
            },
            other => panic!("expected Eq, got {other:?}"),
        }
    }

    #[test]
    fn precedence_and_or() {
        // true || false && false  →  true || (false && false)
        let e = parse("true || false && false").unwrap();
        match e {
            Expr::Binary { op: BinOp::Or, rhs, .. } => {
                assert!(matches!(*rhs, Expr::Binary { op: BinOp::And, .. }));
            }
            other => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn precedence_comparison_vs_contains() {
        // 'a' contains 'a' == true  →  ('a' contains 'a') == true
        let e = parse("'a' contains 'a' == true").unwrap();
        match e {
            Expr::Binary { op: BinOp::Eq, lhs, .. } => {
                assert!(matches!(*lhs, Expr::Binary { op: BinOp::Contains, .. }));
            }
            other => panic!("expected Eq, got {other:?}"),
        }
    }

    #[test]
    fn recursion_limit() {
        let src = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        let result = parse(&src);
        assert!(matches!(result, Err(ExprError::RecursionLimitExceeded { .. })));
    }

    #[test]
    fn recursion_limit_ok() {
        let src = format!("{}1{}", "(".repeat(30), ")".repeat(30));
        assert!(parse(&src).is_ok());
    }

    #[test]
    fn nested_access_ast() {
        let e = parse("user.roles[1]").unwrap();
        match e {
            Expr::Index { obj, .. } => {
                assert!(matches!(*obj, Expr::Member { .. }));
            }
            other => panic!("expected Index, got {other:?}"),
        }
    }

    #[test]
    fn ternary_ast() {
        let e = parse("a ? b : c").unwrap();
        assert!(matches!(e, Expr::Ternary { .. }));
    }

    #[test]
    fn unexpected_token_error() {
        let result = parse("1 +");
        assert!(matches!(result, Err(ExprError::UnexpectedToken { .. })));
    }
}
