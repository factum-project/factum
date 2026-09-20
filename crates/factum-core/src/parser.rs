//! Hand-written recursive-descent parser for Factum S-expression syntax.
//!
//! ## Grammar (EBNF)
//! ```text
//! node       := "(" "node" id node-body* ")"
//! node-body  := ":pred" predicate
//!             | ":valid" validity
//!             | ":src" provenance
//!             | ":conf" number
//!             | ":auth" number
//!             | ":deps" id-list
//!             | ":perm" tag
//! predicate  := "(" symbol term* named-arg* ")"
//!             | "(" ")"  // empty (invalid but produces good error)
//! term       := var | entity | literal | predicate | list
//! literal    := number | string | date | duration | bool | uri
//! named-arg  := ":" symbol term
//! list       := "[" term* "]"
//! validity   := "forever" | "(" "window" date date? ")"
//! provenance := "(" "verbatim" doc span ")"
//!             | "(" "summary" doc span ")"
//!             | "(" "extracted" doc span model ")"
//!             | "(" "derived" from rule ")"
//!             | "(" "asserted" by ")"
//! ```
//!
//! ## Parsing Uniqueness
//! Two rules guarantee parse uniqueness:
//! 1. Full parenthesization (no infix operator precedence)
//! 2. Named arguments must appear after positional arguments

use smol_str::SmolStr;
use chrono::NaiveDate;
use crate::types::*;
use crate::lexer::{Lexer, Token, TokenKind};

/// Maximum nesting depth for recursive-descent parsing.
///
/// This prevents stack overflow DoS on deeply nested input like
/// `((((((((...))))))))`. 128 is generous for legitimate Factum-F
/// (typical nodes nest 2-3 levels deep) while staying well under
/// the default 8MB stack limit on most platforms.
pub const MAX_PARSE_DEPTH: u32 = 128;

/// Maximum number of tokens the lexer will produce before erroring.
/// Prevents OOM on pathologically long input.
pub const MAX_TOKENS: usize = 1_000_000;

/// Parse error with location info.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub line: u32,
    pub col: u32,
    pub offset: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parse error at line {} col {}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

/// The parser. Consumes a token stream and produces AST nodes.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: u32,
}

impl Parser {
    /// Parse a full source string into a list of nodes.
    pub fn parse(src: &str) -> Result<Vec<Node>, ParseError> {
        let tokens = Lexer::new(src).tokenize().map_err(|e| ParseError {
            message: e.message,
            line: e.line,
            col: e.col,
            offset: e.offset,
        })?;

        // Guard: reject pathologically large token streams
        if tokens.len() > MAX_TOKENS {
            return Err(ParseError {
                message: format!("input exceeds maximum token count ({})", MAX_TOKENS),
                line: 0, col: 0, offset: 0,
            });
        }

        let mut p = Self { tokens, pos: 0, depth: 0 };
        let mut nodes = Vec::new();
        while !p.is_at_end() {
            if p.peek_kind() == &TokenKind::LParen {
                if let Some(TokenKind::Symbol(s)) = p.peek_kind_at(1) {
                    if s == "node" {
                        nodes.push(p.parse_node()?);
                        continue;
                    }
                }
                // Skip unknown top-level forms
                p.skip_sexp()?;
            } else {
                p.advance(); // skip stray tokens
            }
        }
        Ok(nodes)
    }

    /// Parse a single predicate from a string.
    pub fn parse_predicate(src: &str) -> Result<Predicate, ParseError> {
        let tokens = Lexer::new(src).tokenize().map_err(|e| ParseError {
            message: e.message, line: e.line, col: e.col, offset: e.offset,
        })?;
        if tokens.len() > MAX_TOKENS {
            return Err(ParseError {
                message: format!("input exceeds maximum token count ({})", MAX_TOKENS),
                line: 0, col: 0, offset: 0,
            });
        }
        let mut p = Self { tokens, pos: 0, depth: 0 };
        p.expect(&TokenKind::LParen)?;
        let pred = p.parse_predicate_body()?;
        p.expect(&TokenKind::RParen)?;
        Ok(pred)
    }

    // ── Internal helpers ──

    fn is_at_end(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_kind_at(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + offset).map(|t| &t.kind)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if !matches!(tok.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<&Token, ParseError> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind) {
            Ok(self.advance())
        } else {
            // Special-case: expected RParen but got EOF → unbalanced parentheses
            if matches!(kind, TokenKind::RParen) && matches!(self.peek_kind(), TokenKind::Eof) {
                Err(self.error("UnbalancedParen: unexpected end of input, missing closing ')'"))
            } else {
                Err(self.error(format!("expected {:?}, got {:?}", kind, self.peek_kind())))
            }
        }
    }

    fn error(&self, msg: impl Into<String>) -> ParseError {
        let tok = &self.tokens[self.pos];
        ParseError {
            message: msg.into(),
            line: tok.line,
            col: tok.col,
            offset: tok.offset,
        }
    }

    /// Check that we haven't exceeded the maximum recursion depth.
    /// Call this at the entry of every recursive method.
    fn check_depth(&self) -> Result<(), ParseError> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(self.error(format!(
                "DepthLimitExceeded: maximum nesting depth ({}) exceeded — possible malformed or adversarial input",
                MAX_PARSE_DEPTH
            )));
        }
        Ok(())
    }

    /// Run a closure with depth incremented by 1.
    fn with_depth<R>(&mut self, f: impl FnOnce(&mut Self) -> Result<R, ParseError>) -> Result<R, ParseError> {
        self.check_depth()?;
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }

    fn skip_sexp(&mut self) -> Result<(), ParseError> {
        // Skip a balanced s-expression
        if self.peek_kind() == &TokenKind::LParen || self.peek_kind() == &TokenKind::LBracket {
            let close = if self.peek_kind() == &TokenKind::LParen { TokenKind::RParen } else { TokenKind::RBracket };
            self.advance();
            let mut depth = 1;
            while depth > 0 && !self.is_at_end() {
                match self.peek_kind() {
                    TokenKind::LParen | TokenKind::LBracket => { depth += 1; self.advance(); }
                    TokenKind::RParen | TokenKind::RBracket => {
                        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(&close) {
                            depth -= 1;
                        }
                        self.advance();
                    }
                    _ => { self.advance(); }
                }
            }
        } else {
            self.advance();
        }
        Ok(())
    }

    // ── Node parsing ──

    fn parse_node(&mut self) -> Result<Node, ParseError> {
        self.with_depth(|p| {
            p.expect(&TokenKind::LParen)?;
            p.expect_symbol("node")?;

            // Node ID
            let id_tok = p.advance().clone();
            let id = match &id_tok.kind {
                TokenKind::Symbol(s) => NodeId::new(s.as_str()),
                _ => return Err(p.error("expected node id")),
            };

            // Parse node body (keyword arguments)
            let mut predicate: Option<Predicate> = None;
            let mut validity = Validity::default();
            let mut provenance = Provenance::default();
            let mut confidence = Confidence::default();
            let mut authority = Authority::default();
            let mut permissions = PermissionTag::PUBLIC;
            let mut deps = Vec::new();
            let mut note: Option<SmolStr> = None;

            while !matches!(p.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
                if let TokenKind::Keyword(k) = p.peek_kind().clone() {
                    p.advance(); // consume keyword
                    match k.as_str() {
                        "pred" => {
                            predicate = Some(p.parse_predicate_form()?);
                        }
                        "valid" => {
                            validity = p.parse_validity()?;
                        }
                        "src" => {
                            provenance = p.parse_provenance()?;
                        }
                        "conf" => {
                            let v = p.parse_f32()?;
                            confidence = Confidence::new(v).map_err(|e| p.error(e.to_string()))?;
                        }
                        "auth" => {
                            let v = p.parse_f32()?;
                            authority = Authority::new(v).map_err(|e| p.error(e.to_string()))?;
                        }
                        "perm" => {
                            permissions = p.parse_permission_tag()?;
                        }
                        "deps" => {
                            deps = p.parse_id_list()?;
                        }
                        "note" => {
                            let tok = p.advance().clone();
                            match &tok.kind {
                                TokenKind::Str(s) => note = Some(s.clone()),
                                TokenKind::Symbol(s) => note = Some(s.clone()),
                                _ => return Err(p.error("expected string or symbol for :note field")),
                            }
                        }
                        _ => return Err(p.error(format!(
                            "UnknownNodeField: unknown node field :{}. Valid fields: :pred, :valid, :src, :conf, :auth, :perm, :deps, :note",
                            k
                        ))),
                    }
                } else {
                    return Err(p.error(format!("expected keyword argument, got {:?}", p.peek_kind())));
                }
            }

            p.expect(&TokenKind::RParen)?;

            let predicate = predicate.ok_or_else(|| p.error("MissingPredField: node missing :pred field"))?;

            Ok(Node {
                id,
                predicate,
                validity,
                provenance,
                confidence,
                authority,
                permissions,
                deps,
                status: NodeStatus::Active,
                note,
            })
        })
    }

    fn expect_symbol(&mut self, expected: &str) -> Result<(), ParseError> {
        if let TokenKind::Symbol(s) = self.peek_kind() {
            if s == expected {
                self.advance();
                return Ok(());
            }
        }
        Err(self.error(format!("expected symbol '{}'", expected)))
    }

    fn parse_f32(&mut self) -> Result<f32, ParseError> {
        let tok = self.advance().clone();
        match &tok.kind {
            TokenKind::Number(s) => s.parse::<f32>().map_err(|e| self.error(e.to_string())),
            _ => Err(self.error("expected number")),
        }
    }

    fn parse_permission_tag(&mut self) -> Result<PermissionTag, ParseError> {
        let tok = self.advance().clone();
        match &tok.kind {
            TokenKind::Symbol(s) => match s.as_str() {
                "public" => Ok(PermissionTag::PUBLIC),
                "internal" => Ok(PermissionTag::INTERNAL),
                "confidential" => Ok(PermissionTag::CONFIDENTIAL),
                "restricted" => Ok(PermissionTag::RESTRICTED),
                _ => Err(self.error(format!("unknown permission tag: {}", s))),
            },
            _ => Err(self.error("expected permission tag symbol")),
        }
    }

    fn parse_id_list(&mut self) -> Result<Vec<NodeId>, ParseError> {
        // Either a single id or a list [id1 id2 ...]
        if self.peek_kind() == &TokenKind::LBracket {
            self.advance();
            let mut ids = Vec::new();
            while !matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
                let tok = self.advance().clone();
                if let TokenKind::Symbol(s) = &tok.kind {
                    ids.push(NodeId::new(s.as_str()));
                } else {
                    return Err(self.error("expected node id in deps list"));
                }
            }
            self.expect(&TokenKind::RBracket)?;
            Ok(ids)
        } else {
            let tok = self.advance().clone();
            if let TokenKind::Symbol(s) = &tok.kind {
                Ok(vec![NodeId::new(s.as_str())])
            } else {
                Err(self.error("expected node id"))
            }
        }
    }

    // ── Predicate parsing ──

    fn parse_predicate_form(&mut self) -> Result<Predicate, ParseError> {
        // :pred (head args...)
        self.with_depth(|p| {
            p.expect(&TokenKind::LParen)?;
            let pred = p.parse_predicate_body_inner()?;
            p.expect(&TokenKind::RParen)?;
            Ok(pred)
        })
    }

    /// Inner predicate body parsing (called within with_depth context).
    fn parse_predicate_body_inner(&mut self) -> Result<Predicate, ParseError> {
        // Head: symbol
        let head_tok = self.advance().clone();
        let head = match &head_tok.kind {
            TokenKind::Symbol(s) => PredicateHead::Name(s.clone()),
            _ => return Err(self.error("expected predicate head symbol")),
        };

        let mut args = Vec::new();
        let mut named = Vec::new();
        let mut in_named = false;

        while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
            if let TokenKind::Keyword(k) = &self.peek_kind().clone() {
                // Named argument
                in_named = true;
                let key = k.clone();
                self.advance();
                let val = self.parse_term()?;
                named.push((key, val));
            } else {
                if in_named {
                    return Err(self.error("NamedArgBeforePositional: positional argument after named argument — named args must be last"));
                }
                args.push(self.parse_term()?);
            }
        }

        Ok(Predicate { head, args, named })
    }

    fn parse_predicate_body(&mut self) -> Result<Predicate, ParseError> {
        self.with_depth(|p| p.parse_predicate_body_inner())
    }

    // ── Term parsing ──

    fn parse_term(&mut self) -> Result<Term, ParseError> {
        // LParen and LBracket branches recurse (via parse_predicate_body or
        // parse_term itself), so we wrap those in with_depth.
        let tok = self.peek_kind().clone();
        match &tok {
            TokenKind::Var(s) => {
                self.advance();
                Ok(Term::Var(s.clone()))
            }
            TokenKind::Entity(s) => {
                self.advance();
                Ok(Term::Ent(EntityId::new(s.as_str())))
            }
            TokenKind::Number(s) => {
                self.advance();
                let lit = Literal::dec_from_str(s).map_err(|e| self.error(e.to_string()))?;
                Ok(Term::Lit(lit))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Term::Lit(Literal::Str(s.clone())))
            }
            TokenKind::Bool(b) => {
                self.advance();
                Ok(Term::Lit(Literal::Bool(*b)))
            }
            TokenKind::Date(s) => {
                self.advance();
                let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| self.error(e.to_string()))?;
                Ok(Term::Lit(Literal::Date(d)))
            }
            TokenKind::Duration(s) => {
                self.advance();
                // Parse duration like "30d", "12h", "60000000000ns"
                let dur = parse_duration(s).map_err(|e| self.error(e))?;
                Ok(Term::Lit(Literal::Dur(dur)))
            }
            TokenKind::Uri(s) => {
                self.advance();
                Ok(Term::Lit(Literal::Uri(s.clone())))
            }
            TokenKind::LParen => {
                // Nested predicate — depth-checked via parse_predicate_body
                self.with_depth(|p| {
                    p.advance();
                    let pred = p.parse_predicate_body_inner()?;
                    p.expect(&TokenKind::RParen)?;
                    Ok(Term::Compound(Box::new(pred)))
                })
            }
            TokenKind::LBracket => {
                // List — depth-checked because list items can nest
                self.with_depth(|p| {
                    p.advance();
                    let mut items = Vec::new();
                    while !matches!(p.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
                        items.push(p.parse_term()?);
                    }
                    p.expect(&TokenKind::RBracket)?;
                    Ok(Term::List(items))
                })
            }
            TokenKind::Symbol(s) => {
                // Symbols in term position are treated as entity references
                // (for cases like ACME-CORP without @ prefix)
                self.advance();
                Ok(Term::Ent(EntityId::new(s.as_str())))
            }
            _ => Err(self.error(format!("unexpected token in term position: {:?}", tok))),
        }
    }

    // ── Validity parsing ──

    fn parse_validity(&mut self) -> Result<Validity, ParseError> {
        let tok = self.peek_kind().clone();
        match &tok {
            TokenKind::Symbol(s) if s == "forever" => {
                self.advance();
                Ok(Validity::Forever)
            }
            TokenKind::LParen => {
                self.advance();
                self.expect_symbol("window")?;
                let from = self.parse_datetime()?;
                let until = if !matches!(self.peek_kind(), TokenKind::RParen) {
                    Some(self.parse_datetime()?)
                } else {
                    None
                };
                self.expect(&TokenKind::RParen)?;
                Ok(Validity::Window { from, until })
            }
            _ => Err(self.error("expected 'forever' or '(window ...)' for validity")),
        }
    }

    fn parse_datetime(&mut self) -> Result<chrono::DateTime<chrono::Utc>, ParseError> {
        let tok = self.advance().clone();
        match &tok.kind {
            TokenKind::Date(s) => {
                let nd = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| self.error(e.to_string()))?;
                Ok(nd.and_hms_opt(0, 0, 0).unwrap().and_utc())
            }
            TokenKind::Str(s) => {
                s.parse::<chrono::DateTime<chrono::Utc>>().map_err(|e| self.error(e.to_string()))
            }
            _ => Err(self.error("expected date or datetime string")),
        }
    }

    // ── Provenance parsing ──

    fn parse_provenance(&mut self) -> Result<Provenance, ParseError> {
        self.expect(&TokenKind::LParen)?;
        let kind_tok = self.advance().clone();
        let kind = match &kind_tok.kind {
            TokenKind::Symbol(s) => s.as_str(),
            _ => return Err(self.error("expected provenance kind symbol")),
        };

        let result = match kind {
            "verbatim" => {
                let doc = self.parse_doc_id()?;
                let span = self.parse_span()?;
                Provenance::Verbatim { doc, span }
            }
            "summary" => {
                let doc = self.parse_doc_id()?;
                let span = self.parse_span()?;
                Provenance::Summary { doc, span }
            }
            "extracted" => {
                let doc = self.parse_doc_id()?;
                let span = self.parse_span()?;
                let model = self.parse_model_ref()?;
                Provenance::Extracted { doc, span, model }
            }
            "derived" => {
                let from_tok = self.advance().clone();
                let from = match &from_tok.kind {
                    TokenKind::Symbol(s) => NodeId::new(s.as_str()),
                    _ => return Err(self.error("expected node id for derived from")),
                };
                let rule = self.parse_rule_id()?;
                Provenance::Derived { from, rule }
            }
            "asserted" => {
                let by = self.parse_principal()?;
                Provenance::Asserted { by }
            }
            _ => return Err(self.error(format!("unknown provenance kind: {}", kind))),
        };

        self.expect(&TokenKind::RParen)?;
        Ok(result)
    }

    fn parse_doc_id(&mut self) -> Result<DocId, ParseError> {
        let tok = self.advance().clone();
        match &tok.kind {
            TokenKind::Symbol(s) | TokenKind::Str(s) => Ok(DocId::new(s.as_str())),
            _ => Err(self.error("expected document id")),
        }
    }

    fn parse_span(&mut self) -> Result<Span, ParseError> {
        // span is [start end] or (span start end)
        if self.peek_kind() == &TokenKind::LBracket {
            self.advance();
            let start = self.parse_u32()?;
            let end = self.parse_u32()?;
            self.expect(&TokenKind::RBracket)?;
            Ok(Span { start, end })
        } else {
            let start = self.parse_u32()?;
            let end = self.parse_u32()?;
            Ok(Span { start, end })
        }
    }

    fn parse_u32(&mut self) -> Result<u32, ParseError> {
        let tok = self.advance().clone();
        match &tok.kind {
            TokenKind::Number(s) => s.parse::<u32>().map_err(|e| self.error(e.to_string())),
            _ => Err(self.error("expected number")),
        }
    }

    fn parse_model_ref(&mut self) -> Result<ModelRef, ParseError> {
        // model is (model name version) or name+version tokens
        // Extracted provenance MUST carry model — this is non-optional.
        if self.peek_kind() == &TokenKind::LParen {
            self.advance();
            self.expect_symbol("model")?;
            let name = self.parse_smol_str()?;
            let version = self.parse_smol_str()?;
            self.expect(&TokenKind::RParen)?;
            Ok(ModelRef { name, version })
        } else if matches!(self.peek_kind(), TokenKind::Symbol(_) | TokenKind::Str(_)) {
            let name = self.parse_smol_str()?;
            let version = self.parse_smol_str()?;
            Ok(ModelRef { name, version })
        } else {
            // The next token is not a valid model ref start — likely a missing model field
            Err(self.error("MissingModelRef: Extracted provenance must carry model reference (model name version) or (model name version)"))
        }
    }

    fn parse_rule_id(&mut self) -> Result<RuleId, ParseError> {
        let s = self.parse_smol_str()?;
        Ok(RuleId(s))
    }

    fn parse_principal(&mut self) -> Result<Principal, ParseError> {
        let s = self.parse_smol_str()?;
        Ok(Principal(s))
    }

    fn parse_smol_str(&mut self) -> Result<SmolStr, ParseError> {
        let tok = self.advance().clone();
        match &tok.kind {
            TokenKind::Symbol(s) | TokenKind::Str(s) => Ok(s.clone()),
            _ => Err(self.error("expected string or symbol")),
        }
    }
}

/// Parse a duration string like "30d", "12h", "5m", "10s", "60000000000ns".
fn parse_duration(s: &str) -> Result<std::time::Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".into());
    }

    // Try nanoseconds first
    if let Some(rest) = s.strip_suffix("ns") {
        let ns: u64 = rest.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        return Ok(std::time::Duration::from_nanos(ns));
    }

    // Extract numeric part and unit
    let mut split = s.len();
    for (i, c) in s.chars().enumerate() {
        if !c.is_ascii_digit() {
            split = i;
            break;
        }
    }

    let (num_str, unit) = s.split_at(split);
    let num: u64 = num_str.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;

    let secs = match unit {
        "s" => num,
        "m" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        _ => return Err(format!("unknown duration unit: {}", unit)),
    };

    Ok(std::time::Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_node() {
        let src = r#"
            (node n001
              :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73)
              :conf 0.85
              :auth 0.9
              :perm confidential)
        "#;
        let nodes = Parser::parse(src).unwrap();
        assert_eq!(nodes.len(), 1);

        let node = &nodes[0];
        assert_eq!(node.id.as_str(), "n001");
        assert_eq!(node.confidence, Confidence(0.85));
        assert_eq!(node.authority, Authority(0.9));
        assert_eq!(node.permissions, PermissionTag::CONFIDENTIAL);

        // Check predicate
        if let PredicateHead::Name(name) = &node.predicate.head {
            assert_eq!(name, "shareholder-major");
        }
        assert_eq!(node.predicate.args.len(), 3);
        assert_eq!(node.predicate.args[0], Term::Ent(EntityId::new("ACME-CORP")));
    }

    #[test]
    fn test_parse_node_with_named_args() {
        let src = r#"
            (node n002
              :pred (revenue @ACME-CORP 230500000000 :period #date(2023-01-01) :currency "CNY"))
        "#;
        let nodes = Parser::parse(src).unwrap();
        let node = &nodes[0];
        assert_eq!(node.predicate.named.len(), 2);
        assert_eq!(node.predicate.named[0].0, "period");
    }

    #[test]
    fn test_parse_node_with_validity() {
        let src = r#"
            (node n003
              :pred (ceo-of @TIM_COOK @APPLE :since #date(2011-08-24))
              :valid (window #date(2011-08-24) #date(2099-12-31)))
        "#;
        let nodes = Parser::parse(src).unwrap();
        let node = &nodes[0];
        match node.validity {
            Validity::Window { from, until } => {
                assert!(until.is_some());
                let from_date = from.date_naive();
                assert_eq!(from_date, NaiveDate::from_ymd_opt(2011, 8, 24).unwrap());
            }
            Validity::Forever => panic!("expected window"),
        }
    }

    #[test]
    fn test_parse_provenance_extracted() {
        let src = r#"
            (node n004
              :pred (located-in @ACME-CORP @ACME-HQ)
              :src (extracted "doc001" [0 100] (model "gpt-4" "2024-06")))
        "#;
        let nodes = Parser::parse(src).unwrap();
        let node = &nodes[0];
        match &node.provenance {
            Provenance::Extracted { doc, span, model } => {
                assert_eq!(&*doc.0, "doc001");
                assert_eq!(span.start, 0);
                assert_eq!(span.end, 100);
                assert_eq!(model.name, "gpt-4");
                assert_eq!(model.version, "2024-06");
            }
            _ => panic!("expected Extracted provenance"),
        }
    }

    #[test]
    fn test_parse_provenance_asserted() {
        let src = r#"
            (node n005
              :pred (founded-on @ACME-CORP #date(2001-03-15))
              :src (asserted admin))
        "#;
        let nodes = Parser::parse(src).unwrap();
        match &nodes[0].provenance {
            Provenance::Asserted { by } => assert_eq!(&*by.0, "admin"),
            _ => panic!("expected Asserted"),
        }
    }

    #[test]
    fn test_parse_node_with_deps() {
        let src = r#"
            (node n006
              :pred (subsidiary-of @ACME-SUB @ACME-CORP)
              :src (derived n001 rule-merge)
              :deps [n001 n002])
        "#;
        let nodes = Parser::parse(src).unwrap();
        let node = &nodes[0];
        assert_eq!(node.deps.len(), 2);
        assert_eq!(node.deps[0].as_str(), "n001");
        match &node.provenance {
            Provenance::Derived { from, rule } => {
                assert_eq!(from.as_str(), "n001");
                assert_eq!(&*rule.0, "rule-merge");
            }
            _ => panic!("expected Derived"),
        }
    }

    #[test]
    fn test_parse_multiple_nodes() {
        let src = r#"
            (node n001 :pred (instance-of @ACME-CORP organization))
            (node n002 :pred (instance-of @APPLE organization))
            (node n003 :pred (located-in @ACME-CORP @ACME-HQ))
        "#;
        let nodes = Parser::parse(src).unwrap();
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_parse_error_missing_pred() {
        let src = "(node n001 :conf 0.5)";
        let result = Parser::parse(src);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("MissingPredField"));
    }

    #[test]
    fn test_parse_error_named_before_positional() {
        let src = "(node n001 :pred (revenue @X :period #date(2023-01-01) 100))";
        // This should fail because positional arg comes after named arg
        let result = Parser::parse(src);
        assert!(result.is_err(), "expected parse error for positional arg after named arg");
    }

    #[test]
    fn test_parse_predicate_standalone() {
        let pred = Parser::parse_predicate("(shareholder-major @X @Y 0.5)").unwrap();
        if let PredicateHead::Name(n) = &pred.head {
            assert_eq!(n, "shareholder-major");
        }
        assert_eq!(pred.args.len(), 3);
    }

    #[test]
    fn test_parse_nested_predicate() {
        let src = r#"
            (node n007
              :pred (acquired-by @X @Y #date(2024-01-01) (revenue @X 100000000)))
        "#;
        let nodes = Parser::parse(src).unwrap();
        let node = &nodes[0];
        assert_eq!(node.predicate.args.len(), 4);
        // Last arg should be a compound
        match &node.predicate.args[3] {
            Term::Compound(pred) => {
                if let PredicateHead::Name(n) = &pred.head {
                    assert_eq!(n, "revenue");
                }
            }
            _ => panic!("expected compound term"),
        }
    }

    #[test]
    fn test_parse_list_term() {
        let src = r#"
            (node n008
              :pred (stake-holders @X [@A @B @C]))
        "#;
        let nodes = Parser::parse(src).unwrap();
        let node = &nodes[0];
        assert_eq!(node.predicate.args.len(), 2);
        match &node.predicate.args[1] {
            Term::List(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected list term"),
        }
    }

    #[test]
    fn test_parse_comments() {
        let src = r#"
            ; This is a comment
            (node n001 ; inline comment
              :pred (instance-of @X organization)) ; trailing
        "#;
        let nodes = Parser::parse(src).unwrap();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_parse_string_with_escapes() {
        let src = r#"
            (node n001 :pred (note @X "He said \"hello\"\n"))
        "#;
        let nodes = Parser::parse(src).unwrap();
        match &nodes[0].predicate.args[1] {
            Term::Lit(Literal::Str(s)) => {
                assert!(s.contains("hello"));
                assert!(s.contains('\n'));
            }
            _ => panic!("expected string literal"),
        }
    }

    // ── Depth limit / DoS protection tests ──

    #[test]
    fn test_depth_limit_nested_predicates() {
        // Build deeply nested compound predicates: (a (a (a ... )))
        let depth = 200; // exceeds MAX_PARSE_DEPTH (128)
        let mut src = String::new();
        for _ in 0..depth {
            src.push_str("(a ");
        }
        src.push_str("@X");
        for _ in 0..depth {
            src.push(')');
        }
        let src = format!("(node n001 :pred {})", src);

        let result = Parser::parse(&src);
        assert!(result.is_err(), "should reject deeply nested input");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("DepthLimitExceeded") || err.message.contains("depth") || err.message.contains("nesting"),
            "error should mention depth: got '{}'", err.message
        );
    }

    #[test]
    fn test_depth_limit_nested_lists() {
        // Build deeply nested lists: [[[[ ... ]]]]
        let depth = 200;
        let mut src = String::new();
        src.push_str("(node n001 :pred (data ");
        for _ in 0..depth {
            src.push('[');
        }
        src.push_str("@X");
        for _ in 0..depth {
            src.push(']');
        }
        src.push(')');

        let result = Parser::parse(&src);
        assert!(result.is_err(), "should reject deeply nested lists");
    }

    #[test]
    fn test_depth_limit_allows_normal_nesting() {
        // Normal nesting (3-4 levels) should work fine
        let src = r#"
            (node n001
              :pred (acquired-by @X @Y
                (revenue @X 1000
                  :period (q4 #date(2024-01-01)))))
        "#;
        let result = Parser::parse(src);
        assert!(result.is_ok(), "normal nesting should parse fine");
    }

    #[test]
    fn test_unterminated_paren_does_not_hang() {
        // Missing closing paren should produce an error, not hang
        let src = "(node n001 :pred (instance-of @X organization";
        let result = Parser::parse(src);
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_node_field_lists_valid_fields() {
        // When a user tries an unknown field like :title or :description,
        // the error should list all valid field names so they can self-correct.
        let src = "(node n001 :pred (note @X \"hello\") :title \"My Node\")";
        let result = Parser::parse(src);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("UnknownNodeField"), "should mention UnknownNodeField");
        assert!(err.message.contains(":pred"), "should list :pred as valid");
        assert!(err.message.contains(":valid"), "should list :valid as valid");
        assert!(err.message.contains(":src"), "should list :src as valid");
        assert!(err.message.contains(":conf"), "should list :conf as valid");
        assert!(err.message.contains(":auth"), "should list :auth as valid");
        assert!(err.message.contains(":perm"), "should list :perm as valid");
        assert!(err.message.contains(":deps"), "should list :deps as valid");
        assert!(err.message.contains(":note"), "should list :note as valid");
    }

    #[test]
    fn test_parse_node_with_note() {
        let src = r#"
            (node n009
              :pred (version @PROJECT "6.0")
              :note "this is the v6 plan")
        "#;
        let nodes = Parser::parse(src).unwrap();
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];
        assert_eq!(node.id.as_str(), "n009");
        assert!(node.note.is_some());
        assert_eq!(node.note.as_ref().unwrap(), "this is the v6 plan");
    }

    #[test]
    fn test_parse_node_note_symbol() {
        // Note can also be a symbol (unquoted)
        let src = "(node n010 :pred (status @X active) :note quick-reminder)";
        let nodes = Parser::parse(src).unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].note.is_some());
        assert_eq!(nodes[0].note.as_ref().unwrap(), "quick-reminder");
    }

    #[test]
    fn test_roundtrip_with_note() {
        // note should survive canonical round-trip
        let src = r#"(node n011 :pred (version @X "1.0") :note "test roundtrip")"#;
        let nodes = Parser::parse(src).unwrap();
        let canon = crate::serialize::canonical(&nodes[0]);
        let reparsed = Parser::parse(&canon).unwrap();
        assert_eq!(reparsed.len(), 1);
        assert!(reparsed[0].note.is_some());
        assert_eq!(reparsed[0].note.as_ref().unwrap(), "test roundtrip");
    }
}
