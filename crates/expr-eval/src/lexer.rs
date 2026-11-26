//! UTF-8 safe lexer (tokenizer) for the `expr_eval` expression engine.
//!
//! Produces a flat sequence of [`Token`]s with precise source positions.

use crate::error::{ExprError};

/// A lexical token produced by the lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A string literal (quotes already stripped, escapes resolved).
    Str(String),
    /// A boolean `true`.
    BoolTrue,
    /// A boolean `false`.
    BoolFalse,
    /// The `null` keyword.
    Null,
    /// An identifier (variable name or function name).
    Ident(String),
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `==`
    EqEq,
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
    /// `!`
    Not,
    /// `?`
    Question,
    /// `:`
    Colon,
    /// `.`
    Dot,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// The `contains` keyword operator.
    Contains,
    /// The `starts_with` keyword operator.
    StartsWith,
    /// The `ends_with` keyword operator.
    EndsWith,
    /// The `in` keyword operator.
    In,
    /// End of input.
    Eof,
}

impl Token {
    /// Returns a human-readable description (used in error messages).
    pub fn description(&self) -> &'static str {
        match self {
            Token::Int(_) => "integer literal",
            Token::Float(_) => "float literal",
            Token::Str(_) => "string literal",
            Token::BoolTrue => "true",
            Token::BoolFalse => "false",
            Token::Null => "null",
            Token::Ident(_) => "identifier",
            Token::Plus => "+",
            Token::Minus => "-",
            Token::Star => "*",
            Token::Slash => "/",
            Token::Percent => "%",
            Token::EqEq => "==",
            Token::NotEq => "!=",
            Token::Lt => "<",
            Token::LtEq => "<=",
            Token::Gt => ">",
            Token::GtEq => ">=",
            Token::And => "&&",
            Token::Or => "||",
            Token::Not => "!",
            Token::Question => "?",
            Token::Colon => ":",
            Token::Dot => ".",
            Token::LBracket => "[",
            Token::RBracket => "]",
            Token::LParen => "(",
            Token::RParen => ")",
            Token::Comma => ",",
            Token::Contains => "contains",
            Token::StartsWith => "starts_with",
            Token::EndsWith => "ends_with",
            Token::In => "in",
            Token::Eof => "end of input",
        }
    }
}

/// A token annotated with its source position.
#[derive(Debug, Clone)]
pub struct Located {
    /// The token itself.
    pub token: Token,
    /// Zero-based byte offset in the source.
    pub offset: usize,
    /// One-based line number.
    pub line: usize,
    /// One-based column number (in characters).
    pub col: usize,
}

/// The lexer state.
pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    /// Creates a new lexer over the given source string.
    pub fn new(src: &str) -> Self {
        let chars: Vec<char> = src.chars().collect();
        Lexer { chars, pos: 0 }
    }

    /// Tokenizes the entire input, returning all tokens (ending with `Eof`).
    pub fn tokenize(&mut self) -> Result<Vec<Located>, ExprError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            if self.pos >= self.chars.len() {
                tokens.push(Located {
                    token: Token::Eof,
                    offset: self.pos,
                    line: 1,
                    col: self.pos + 1,
                });
                break;
            }
            let (token, offset, line, col) = self.next_token()?;
            tokens.push(Located { token, offset, line, col });
        }
        Ok(tokens)
    }

    fn position(&self) -> (usize, usize, usize) {
        let byte_offset = self.chars[..self.pos].iter().map(|c| c.len_utf8()).sum::<usize>();
        let mut line = 1;
        let mut col = 1;
        for &c in &self.chars[..self.pos] {
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (byte_offset, line, col)
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).copied()
    }

    fn advance(&mut self) -> char {
        let c = self.chars[self.pos];
        self.pos += 1;
        c
    }

    fn next_token(&mut self) -> Result<(Token, usize, usize, usize), ExprError> {
        let (offset, line, col) = self.position();
        let c = self.peek().unwrap();

        // String literals
        if c == '"' || c == '\'' {
            let (val, _end) = self.read_string(c)?;
            return Ok((Token::Str(val), offset, line, col));
        }

        // Numbers
        if c.is_ascii_digit() {
            return self.read_number(offset, line, col);
        }

        // Identifiers and keywords
        if c.is_alphabetic() || c == '_' {
            return self.read_ident_or_keyword(offset, line, col);
        }

        // Operators (check two-char first)
        let c2 = self.peek_at(1);
        if let Some(c2) = c2 {
            match (c, c2) {
                ('=', '=') => { self.pos += 2; return Ok((Token::EqEq, offset, line, col)); }
                ('!', '=') => { self.pos += 2; return Ok((Token::NotEq, offset, line, col)); }
                ('<', '=') => { self.pos += 2; return Ok((Token::LtEq, offset, line, col)); }
                ('>', '=') => { self.pos += 2; return Ok((Token::GtEq, offset, line, col)); }
                ('&', '&') => { self.pos += 2; return Ok((Token::And, offset, line, col)); }
                ('|', '|') => { self.pos += 2; return Ok((Token::Or, offset, line, col)); }
                _ => {}
            }
        }

        match c {
            '+' => { self.pos += 1; Ok((Token::Plus, offset, line, col)) }
            '-' => { self.pos += 1; Ok((Token::Minus, offset, line, col)) }
            '*' => { self.pos += 1; Ok((Token::Star, offset, line, col)) }
            '/' => { self.pos += 1; Ok((Token::Slash, offset, line, col)) }
            '%' => { self.pos += 1; Ok((Token::Percent, offset, line, col)) }
            '<' => { self.pos += 1; Ok((Token::Lt, offset, line, col)) }
            '>' => { self.pos += 1; Ok((Token::Gt, offset, line, col)) }
            '!' => { self.pos += 1; Ok((Token::Not, offset, line, col)) }
            '?' => { self.pos += 1; Ok((Token::Question, offset, line, col)) }
            ':' => { self.pos += 1; Ok((Token::Colon, offset, line, col)) }
            '.' => { self.pos += 1; Ok((Token::Dot, offset, line, col)) }
            '[' => { self.pos += 1; Ok((Token::LBracket, offset, line, col)) }
            ']' => { self.pos += 1; Ok((Token::RBracket, offset, line, col)) }
            '(' => { self.pos += 1; Ok((Token::LParen, offset, line, col)) }
            ')' => { self.pos += 1; Ok((Token::RParen, offset, line, col)) }
            ',' => { self.pos += 1; Ok((Token::Comma, offset, line, col)) }
            other => Err(ExprError::UnexpectedChar {
                character: other,
                offset,
                line,
                col,
            }),
        }
    }

    fn read_string(&mut self, quote: char) -> Result<(String, usize), ExprError> {
        self.pos += 1; // consume opening quote
        let mut buf = String::new();
        loop {
            if self.pos >= self.chars.len() {
                let (offset, line, col) = self.position();
                return Err(ExprError::UnterminatedString {
                    // position of opening quote is offset - 1
                    offset: offset.saturating_sub(1),
                    line,
                    col,
                });
            }
            let c = self.advance();
            match c {
                '\\' => {
                    if self.pos >= self.chars.len() {
                        let (offset, line, col) = self.position();
                        return Err(ExprError::UnterminatedString { offset, line, col });
                    }
                    let esc = self.advance();
                    match esc {
                        'n' => buf.push('\n'),
                        't' => buf.push('\t'),
                        '\\' => buf.push('\\'),
                        '"' => buf.push('"'),
                        '\'' => buf.push('\''),
                        other => {
                            let (offset, line, col) = self.position();
                            return Err(ExprError::UnexpectedChar {
                                character: other,
                                offset,
                                line,
                                col,
                            });
                        }
                    }
                }
                ch if ch == quote => {
                    return Ok((buf, self.pos));
                }
                other => buf.push(other),
            }
        }
    }

    fn read_number(&mut self, offset: usize, line: usize, col: usize) -> Result<(Token, usize, usize, usize), ExprError> {
        let start = self.pos;
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // Check for float
        if self.pos + 1 < self.chars.len() && self.chars[self.pos] == '.' && self.chars[self.pos + 1].is_ascii_digit() {
            self.pos += 1; // consume '.'
            while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            let text: String = self.chars[start..self.pos].iter().collect();
            let val = text.parse::<f64>().map_err(|_| ExprError::UnexpectedToken {
                expected: "valid float".into(),
                found: text.clone(),
                offset,
                line,
                col,
            })?;
            return Ok((Token::Float(val), offset, line, col));
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        let val = text.parse::<i64>().map_err(|_| ExprError::IntegerOverflow {
            offset, line, col,
        })?;
        Ok((Token::Int(val), offset, line, col))
    }

    fn read_ident_or_keyword(&mut self, offset: usize, line: usize, col: usize) -> Result<(Token, usize, usize, usize), ExprError> {
        let start = self.pos;
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c.is_alphanumeric() || c == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let word: String = self.chars[start..self.pos].iter().collect();
        let token = match word.as_str() {
            "true" => Token::BoolTrue,
            "false" => Token::BoolFalse,
            "null" => Token::Null,
            "contains" => Token::Contains,
            "starts_with" => Token::StartsWith,
            "ends_with" => Token::EndsWith,
            "in" => Token::In,
            other => Token::Ident(other.to_owned()),
        };
        Ok((token, offset, line, col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_escapes() {
        let mut lx = Lexer::new("\"hello\\nworld\"");
        let tokens = lx.tokenize().unwrap();
        // tokens: Str("hello\nworld"), Eof
        assert_eq!(tokens.len(), 2);
        match &tokens[0].token {
            Token::Str(s) => {
                assert_eq!(s, "hello\nworld");
            }
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn string_escapes_single_quote() {
        let mut lx = Lexer::new("'it\\'s'");
        let tokens = lx.tokenize().unwrap();
        match &tokens[0].token {
            Token::Str(s) => assert_eq!(s, "it's"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn operators_lexing() {
        let mut lx = Lexer::new("a == b != c <= d >= e && f || g");
        let tokens = lx.tokenize().unwrap();
        let descs: Vec<&str> = tokens.iter().map(|t| t.token.description()).collect();
        assert!(descs.contains(&"=="));
        assert!(descs.contains(&"!="));
        assert!(descs.contains(&"<="));
        assert!(descs.contains(&">="));
        assert!(descs.contains(&"&&"));
        assert!(descs.contains(&"||"));
    }

    #[test]
    fn keyword_operators() {
        let mut lx = Lexer::new("'hello' contains 'ell'");
        let tokens = lx.tokenize().unwrap();
        assert!(matches!(&tokens[1].token, Token::Contains));

        let mut lx2 = Lexer::new("'abc' starts_with 'ab'");
        let tokens2 = lx2.tokenize().unwrap();
        assert!(matches!(&tokens2[1].token, Token::StartsWith));

        let mut lx3 = Lexer::new("'abc' ends_with 'bc'");
        let tokens3 = lx3.tokenize().unwrap();
        assert!(matches!(&tokens3[1].token, Token::EndsWith));

        let mut lx4 = Lexer::new("'a' in ['a','b']");
        let tokens4 = lx4.tokenize().unwrap();
        assert!(matches!(&tokens4[1].token, Token::In));
    }

    #[test]
    fn unterminated_string_error() {
        let mut lx = Lexer::new("\"abc");
        let result = lx.tokenize();
        assert!(matches!(result, Err(ExprError::UnterminatedString { .. })));
    }

    #[test]
    fn unexpected_char_error() {
        let mut lx = Lexer::new("1 + @");
        let result = lx.tokenize();
        assert!(matches!(result, Err(ExprError::UnexpectedChar { character: '@', .. })));
    }
}