//! S-expression lexer for Factum.
//!
//! Tokenizes Factum source into tokens for the recursive-descent parser.
//! Tokens include: parens, symbols, strings, numbers, keywords, variables.

use smol_str::SmolStr;

/// Token kinds in Factum S-expression syntax.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// A symbol like `node`, `shareholder-major`, `ACME-CORP`
    Symbol(SmolStr),
    /// A variable like `?p`, `?x`, `?now`
    Var(SmolStr),
    /// A keyword argument like `:valid`, `:src`, `:conf`
    Keyword(SmolStr),
    /// A string literal "..."
    Str(SmolStr),
    /// A decimal number (stored as string for lossless parsing)
    Number(SmolStr),
    /// A date #date(2024-01-15)
    Date(SmolStr),
    /// A duration #dur(30d)
    Duration(SmolStr),
    /// A boolean #t or #f
    Bool(bool),
    /// A URI <https://...>
    Uri(SmolStr),
    /// An entity @EntityName
    Entity(SmolStr),
    /// EOF
    Eof,
}

/// A token with position information.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// Byte offset in source.
    pub offset: usize,
    /// Line number (1-based).
    pub line: u32,
    /// Column number (1-based).
    pub col: u32,
}

/// Lexing error.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub offset: usize,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lex error at line {} col {}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for LexError {}

/// The lexer. Converts source text into a Vec<Token>.
pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    /// Tokenize the entire source string.
    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.pos >= self.src.len() {
                tokens.push(self.make_token(TokenKind::Eof));
                break;
            }
            // Guard: prevent OOM from pathologically long input
            if tokens.len() >= 1_000_000 {
                return Err(LexError {
                    message: "input exceeds maximum token count (1000000)".into(),
                    offset: self.pos,
                    line: self.line,
                    col: self.col,
                });
            }
            let tok = self.next_token()?;
            tokens.push(tok);
        }
        Ok(tokens)
    }

    fn make_token(&self, kind: TokenKind) -> Token {
        Token {
            kind,
            offset: self.pos,
            line: self.line,
            col: self.col,
        }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            // Whitespace
            while self.pos < self.src.len() {
                let c = self.src[self.pos];
                if c == b' ' || c == b'\t' || c == b'\r' {
                    self.advance();
                } else if c == b'\n' {
                    self.advance();
                    self.line += 1;
                    self.col = 1;
                } else {
                    break;
                }
            }
            // Comments: ; until end of line
            if self.pos < self.src.len() && self.src[self.pos] == b';' {
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.advance();
                }
                continue; // might have trailing ws after comment
            }
            break;
        }
    }

    fn advance(&mut self) {
        self.pos += 1;
        self.col += 1;
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        let start_offset = self.pos;
        let start_line = self.line;
        let start_col = self.col;

        let c = self.src[self.pos];

        match c {
            b'(' => { self.advance(); Ok(Token { kind: TokenKind::LParen, offset: start_offset, line: start_line, col: start_col }) }
            b')' => { self.advance(); Ok(Token { kind: TokenKind::RParen, offset: start_offset, line: start_line, col: start_col }) }
            b'[' => { self.advance(); Ok(Token { kind: TokenKind::LBracket, offset: start_offset, line: start_line, col: start_col }) }
            b']' => { self.advance(); Ok(Token { kind: TokenKind::RBracket, offset: start_offset, line: start_line, col: start_col }) }
            b'"' => self.lex_string(start_offset, start_line, start_col),
            b'?' => self.lex_var(start_offset, start_line, start_col),
            b':' => self.lex_keyword(start_offset, start_line, start_col),
            b'<' => self.lex_uri(start_offset, start_line, start_col),
            b'@' => self.lex_entity(start_offset, start_line, start_col),
            b'#' => self.lex_hash_prefixed(start_offset, start_line, start_col),
            _ if c.is_ascii_digit() || (c == b'-' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit())) => {
                self.lex_number(start_offset, start_line, start_col)
            }
            _ => self.lex_symbol(start_offset, start_line, start_col),
        }
    }

    fn lex_string(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        self.advance(); // skip opening "
        let mut buf = String::new();
        loop {
            match self.peek() {
                None => return Err(LexError {
                    message: "UnterminatedString: string literal without closing quote".into(),
                    offset: start_offset,
                    line: start_line,
                    col: start_col,
                }),
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance();
                    match self.peek() {
                        Some(b'n') => { buf.push('\n'); self.advance(); }
                        Some(b't') => { buf.push('\t'); self.advance(); }
                        Some(b'r') => { buf.push('\r'); self.advance(); }
                        Some(b'\\') => { buf.push('\\'); self.advance(); }
                        Some(b'"') => { buf.push('"'); self.advance(); }
                        Some(c) => { buf.push(c as char); self.advance(); }
                        None => return Err(LexError {
                            message: "unterminated string escape".into(),
                            offset: self.pos,
                            line: self.line,
                            col: self.col,
                        }),
                    }
                }
                Some(c) => {
                    // Read full UTF-8 char
                    if c < 0x80 {
                        buf.push(c as char);
                        self.advance();
                    } else {
                        let utf8 = self.read_utf8_char()?;
                        buf.push(utf8);
                    }
                }
            }
        }
        Ok(Token { kind: TokenKind::Str(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
    }

    fn read_utf8_char(&mut self) -> Result<char, LexError> {
        let start = self.pos;
        let len = if self.src[self.pos] < 0x80 { 1 }
            else if self.src[self.pos] & 0xE0 == 0xC0 { 2 }
            else if self.src[self.pos] & 0xF0 == 0xE0 { 3 }
            else if self.src[self.pos] & 0xF8 == 0xF0 { 4 }
            else { return Err(LexError {
                message: "invalid UTF-8".into(),
                offset: start,
                line: self.line,
                col: self.col,
            }) };

        if start + len > self.src.len() {
            return Err(LexError {
                message: "truncated UTF-8".into(),
                offset: start,
                line: self.line,
                col: self.col,
            });
        }

        let bytes = &self.src[start..start + len];
        let s = std::str::from_utf8(bytes).map_err(|_| LexError {
            message: "invalid UTF-8".into(),
            offset: start,
            line: self.line,
            col: self.col,
        })?;
        let ch = s.chars().next().unwrap();
        for _ in 0..len { self.advance(); }
        Ok(ch)
    }

    fn lex_var(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        self.advance(); // skip ?
        let mut buf = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80 {
                if c >= 0x80 {
                    let ch = self.read_utf8_char()?;
                    buf.push(ch);
                } else {
                    buf.push(c as char);
                    self.advance();
                }
            } else {
                break;
            }
        }
        if buf.is_empty() {
            return Err(LexError {
                message: "variable name expected after ?".into(),
                offset: start_offset,
                line: start_line,
                col: start_col,
            });
        }
        Ok(Token { kind: TokenKind::Var(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
    }

    fn lex_keyword(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        self.advance(); // skip :
        let mut buf = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80 {
                if c >= 0x80 {
                    let ch = self.read_utf8_char()?;
                    buf.push(ch);
                } else {
                    buf.push(c as char);
                    self.advance();
                }
            } else {
                break;
            }
        }
        if buf.is_empty() {
            return Err(LexError {
                message: "keyword name expected after :".into(),
                offset: start_offset,
                line: start_line,
                col: start_col,
            });
        }
        Ok(Token { kind: TokenKind::Keyword(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
    }

    fn lex_uri(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        self.advance(); // skip <
        let mut buf = String::new();
        loop {
            match self.peek() {
                None => return Err(LexError {
                    message: "unterminated URI".into(),
                    offset: start_offset,
                    line: start_line,
                    col: start_col,
                }),
                Some(b'>') => { self.advance(); break; }
                Some(c) => {
                    if c >= 0x80 {
                        let ch = self.read_utf8_char()?;
                        buf.push(ch);
                    } else {
                        buf.push(c as char);
                        self.advance();
                    }
                }
            }
        }
        Ok(Token { kind: TokenKind::Uri(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
    }

    fn lex_entity(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        self.advance(); // skip @
        let mut buf = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b':' || c == b'.' || c >= 0x80 {
                if c >= 0x80 {
                    let ch = self.read_utf8_char()?;
                    buf.push(ch);
                } else {
                    buf.push(c as char);
                    self.advance();
                }
            } else {
                break;
            }
        }
        if buf.is_empty() {
            return Err(LexError {
                message: "entity name expected after @".into(),
                offset: start_offset,
                line: start_line,
                col: start_col,
            });
        }
        Ok(Token { kind: TokenKind::Entity(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
    }

    fn lex_hash_prefixed(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        self.advance(); // skip #
        match self.peek() {
            Some(b't') => { self.advance(); Ok(Token { kind: TokenKind::Bool(true), offset: start_offset, line: start_line, col: start_col }) }
            Some(b'f') => { self.advance(); Ok(Token { kind: TokenKind::Bool(false), offset: start_offset, line: start_line, col: start_col }) }
            Some(b'd') if self.peek_at(1) == Some(b'a') => {
                // #date(YYYY-MM-DD)
                self.advance(); // d
                // Read rest of "ate"
                for expected in *b"ate" {
                    if self.peek() != Some(expected) {
                        return Err(LexError {
                            message: "expected 'date'".into(),
                            offset: start_offset, line: start_line, col: start_col,
                        });
                    }
                    self.advance();
                }
                // Expect (
                if self.peek() != Some(b'(') {
                    return Err(LexError {
                        message: "expected '(' after #date".into(),
                        offset: start_offset, line: start_line, col: start_col,
                    });
                }
                self.advance();
                let mut buf = String::new();
                while let Some(c) = self.peek() {
                    if c == b')' { self.advance(); break; }
                    buf.push(c as char);
                    self.advance();
                }
                Ok(Token { kind: TokenKind::Date(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
            }
            Some(b'd') if self.peek_at(1) == Some(b'u') => {
                // #dur(...)
                self.advance(); // d
                for expected in *b"ur" {
                    if self.peek() != Some(expected) {
                        return Err(LexError {
                            message: "expected 'dur'".into(),
                            offset: start_offset, line: start_line, col: start_col,
                        });
                    }
                    self.advance();
                }
                if self.peek() != Some(b'(') {
                    return Err(LexError {
                        message: "expected '(' after #dur".into(),
                        offset: start_offset, line: start_line, col: start_col,
                    });
                }
                self.advance();
                let mut buf = String::new();
                while let Some(c) = self.peek() {
                    if c == b')' { self.advance(); break; }
                    buf.push(c as char);
                    self.advance();
                }
                Ok(Token { kind: TokenKind::Duration(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
            }
            _ => Err(LexError {
                message: "unknown # prefix (expected #t, #f, #date, #dur)".into(),
                offset: start_offset, line: start_line, col: start_col,
            }),
        }
    }

    fn lex_number(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        let mut buf = String::new();
        if self.peek() == Some(b'-') {
            buf.push('-');
            self.advance();
        }
        // Integer part
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                buf.push(c as char);
                self.advance();
            } else {
                break;
            }
        }
        // Fractional part
        if self.peek() == Some(b'.') {
            buf.push('.');
            self.advance();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    buf.push(c as char);
                    self.advance();
                } else {
                    break;
                }
            }
        }
        // Exponent (stored as decimal, not f64 — we keep the raw string)
        // For now, we don't support exponent notation in Dec.
        // Users should use full decimal notation.

        Ok(Token { kind: TokenKind::Number(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
    }

    fn lex_symbol(&mut self, start_offset: usize, start_line: u32, start_col: u32) -> Result<Token, LexError> {
        let mut buf = String::new();
        while let Some(c) = self.peek() {
            if c == b'(' || c == b')' || c == b'[' || c == b']' || c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == b';' || c == b'"' || c == b':' || c == b'?' || c == b'@' || c == b'<' || c == b'#' {
                break;
            }
            if c >= 0x80 {
                let ch = self.read_utf8_char()?;
                buf.push(ch);
            } else {
                buf.push(c as char);
                self.advance();
            }
        }
        if buf.is_empty() {
            return Err(LexError {
                message: format!("unexpected character: '{}'", self.peek().map(|c| c as char).unwrap_or(' ')),
                offset: start_offset,
                line: start_line,
                col: start_col,
            });
        }
        Ok(Token { kind: TokenKind::Symbol(SmolStr::new(buf)), offset: start_offset, line: start_line, col: start_col })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(src: &str) -> Vec<TokenKind> {
        Lexer::new(src).tokenize().unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn test_basic_tokens() {
        assert_eq!(tokenize("(node n001)"), vec![
            TokenKind::LParen,
            TokenKind::Symbol("node".into()),
            TokenKind::Symbol("n001".into()),
            TokenKind::RParen,
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_string_and_number() {
        let tokens = tokenize(r#"(shareholder-major @ACME-CORP "founder" 0.73)"#);
        assert_eq!(tokens, vec![
            TokenKind::LParen,
            TokenKind::Symbol("shareholder-major".into()),
            TokenKind::Entity("ACME-CORP".into()),
            TokenKind::Str("founder".into()),
            TokenKind::Number("0.73".into()),
            TokenKind::RParen,
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_var_and_keyword() {
        let tokens = tokenize("(pred ?x :valid [now])");
        assert_eq!(tokens, vec![
            TokenKind::LParen,
            TokenKind::Symbol("pred".into()),
            TokenKind::Var("x".into()),
            TokenKind::Keyword("valid".into()),
            TokenKind::LBracket,
            TokenKind::Symbol("now".into()),
            TokenKind::RBracket,
            TokenKind::RParen,
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_comments() {
        let tokens = tokenize("; comment\n(node) ; trailing");
        assert_eq!(tokens.len(), 4); // LParen, Symbol, RParen, Eof
    }

    #[test]
    fn test_bool_and_date() {
        let tokens = tokenize("(active #t #date(2024-01-15) #f)");
        assert_eq!(tokens, vec![
            TokenKind::LParen,
            TokenKind::Symbol("active".into()),
            TokenKind::Bool(true),
            TokenKind::Date("2024-01-15".into()),
            TokenKind::Bool(false),
            TokenKind::RParen,
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_uri() {
        let tokens = tokenize("<https://example.com>");
        assert_eq!(tokens, vec![
            TokenKind::Uri("https://example.com".into()),
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_negative_number() {
        let tokens = tokenize("-5.25");
        assert_eq!(tokens, vec![
            TokenKind::Number("-5.25".into()),
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_unterminated_string() {
        let result = Lexer::new(r#"("hello"#).tokenize();
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("UnterminatedString"));
    }
}
