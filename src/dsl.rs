use std::{borrow::Cow, fmt::Display, ops::Range};
use itertools::Itertools;
use thiserror::Error;
// use std::io::{Write, stderr};
// use itertools::Itertools;
// use crate::song::{Song, read_songs_in_dir, anyhow_songs_to_ok_err};

type Num = u32;

#[derive(Debug, PartialEq)]
enum TokenType<'q> {
    BinOp(&'q str),
    UnOp(&'q str),
    QuotedLit(&'q str),
    Lit(&'q str),
    Number(Num),
    OParen,
    CParen,
}

#[derive(Debug, PartialEq)]
struct Token<'q> {
    ty: TokenType<'q>,
    str: &'q str,
}

#[derive(Debug, Error)]
pub struct MqlError<'q> {
    pub query: &'q str,
    pub range: Range<usize>,
    pub reason: Cow<'q, str>,
}

impl Display for MqlError<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Claude: `range` is in bytes, the terminal column is in chars
        let spaces = " ".repeat(self.query[..self.range.start].chars().count());
        let arrows = "^".repeat(self.query[self.range.start..self.range.end].chars().count());
        writeln!(f)?;
        writeln!(f, "'{}'", self.query)?;
        writeln!(f, " {spaces}{arrows}")?;
        writeln!(f, " {spaces}{}", self.reason)?;
        Ok(())
    }
}

mod tok {
    macro_rules! binop  { ($v: pat) => { Token { ty: TokenType::BinOp($v), .. } }; }
    macro_rules! unop   { ($v: pat) => { Token { ty: TokenType::UnOp($v), .. } }; }
    macro_rules! qlit   { ($v: pat) => { Token { ty: TokenType::QuotedLit($v), .. } }; }
    macro_rules! lit    { ($v: pat) => { Token { ty: TokenType::Lit($v), .. } }; }
    macro_rules! number { ($v: pat) => { Token { ty: TokenType::Number($v), .. } }; }
    macro_rules! oparen { () => { Token { ty: TokenType::OParen, .. } }; }
    macro_rules! cparen { () => { Token { ty: TokenType::CParen, .. } }; }

    pub(crate) use { binop, unop, qlit, lit, number, oparen, cparen };
}

const CMP_NUM_OPS: &[&str] = &[">=", "<=", ">", "<"];
const CMP_SET_OPS: &[&str] = &["==", "is", "!=", "is not"];

const BOOL_UNOPS: &[&str] = &["!", "not"];
const BOOL_BINOPS: &[&str] = &["||", "or", "&&", "and"];

enum OpType {
    Bool(&'static str),
    Num(&'static str),
    Set(&'static str),
}

enum NodeType {
    BinOp {
        ty: OpType,
        lhs_and_rhs: Box<(Node, Node)>,
    },
    UnOp {
        // ty: Bool
        expr: Box<Node>,
    },
    Bool(bool),
}

pub struct Node {
    ty: NodeType,
    parsed: Range<usize>,
}

fn tokenize<'q>(q: &'q str) -> Result<Vec<Token<'q>>, MqlError<'q>> {
    // Sirynka: Stupid language, can't create a comptime set
    let bin_ops = [BOOL_BINOPS, CMP_SET_OPS, CMP_NUM_OPS, &["."]]
        .iter().copied().flatten().collect_vec();

    let consume = |c: usize| -> Result<(Token<'q>, usize), MqlError<'q>> {
        // Claude: `or` must not match the start of `order` (or `orłowski`)
        let matches_op = |c: usize, op: &str| {
            let is_word = op.starts_with(|c: char| c.is_alphabetic());
            q[c..].starts_with(op) && !(is_word && q[c+op.len()..].starts_with(|c: char| c.is_alphanumeric()))
        };
        let tok = |ty, e: usize| Ok((Token { ty, str: &q[c..e] }, e));

        if q[c..].starts_with('(') { return tok(TokenType::OParen, c+1); }
        if q[c..].starts_with(')') { return tok(TokenType::CParen, c+1); }

        // Claude: `is not` is a single op, `str` keeps the whitespace between the words as written
        if matches_op(c, "is") {
            let n = c + 2 + q[c+2..].len() - q[c+2..].trim_start().len();
            if matches_op(n, "not") { return tok(TokenType::BinOp("is not"), n+3); }
        }

        if let Some(op) = bin_ops.iter().find(|op| matches_op(c, op)) { return tok(TokenType::BinOp(op), c+op.len()); }
        if let Some(op) = BOOL_UNOPS.iter().find(|op| matches_op(c, op)) { return tok(TokenType::UnOp(op), c+op.len()); }

        // Claude: a quote glued to a letter or digit is part of a value (`5'nizza`, `'it's'`), not a delimiter.
        // We can't tell where that value ends, so point at the quote and don't guess the fix.
        if let Some(quote @ ('\'' | '"')) = q[c..].chars().next() {
            let is_word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
            let glued = |at: usize| {
                let other = if quote == '\'' { "double" } else { "single" };
                let reason = format!("`{quote}` inside a value, wrap the whole value in {other} quotes");
                MqlError { query: q, reason: Cow::Owned(reason), range: at..at+1 }
            };

            if is_word(q[..c].chars().next_back()) { return Err(glued(c)); }
            let e = c+1 + q[c+1..].find(quote).ok_or(MqlError { query: q, reason: Cow::Borrowed("unterminated quote"), range: c..q.len() })?;
            if is_word(q[e+1..].chars().next()) { return Err(glued(e)); }
            return tok(TokenType::QuotedLit(&q[c+1..e]), e+1);
        }

        // Claude: read the whole word first, so `2pac` and `50cent` are lits, not a number followed by a lit
        if q[c..].starts_with(|c: char| c.is_alphanumeric()) {
            let e = c + q[c..].find(|c: char| !c.is_alphanumeric()).unwrap_or(q.len()-c);
            if !q[c..e].bytes().all(|b| b.is_ascii_digit()) { return tok(TokenType::Lit(&q[c..e]), e); }

            let n = q[c..e].parse().map_err(|err| MqlError { query: q, reason: Cow::Owned(format!("{err}")), range: c..e })?;
            return tok(TokenType::Number(n), e);
        }

        // Claude: range covers the whole char, it may be multi-byte.
        let len = q[c..].chars().next().map_or(1, char::len_utf8);
        Err(MqlError { query: q, reason: Cow::Borrowed("unexpected token"), range: c..c+len })
    };

    let mut c = 0;
    let mut t: Token<'_>;
    let mut toks = Vec::new();

    loop {
        c += q[c..].len() - q[c..].trim_start().len();
        if c == q.len() { break; }
        (t, c) = consume(c)?;
        toks.push(t);
    }

    Ok(toks)
}

fn parse<'q, 't>(q: &'q str, toks: &'t [Token<'q>]) -> Result<Node, MqlError<'q>> {
    unimplemented!()
}

pub struct MQL;
impl<'q, 't> MQL {
    pub fn parse(q: &'q str) -> Result<Node, MqlError<'q>> {
        let toks = tokenize(q)?;
        let ast = parse(q, &toks)?;
        Ok(ast)
    }

    pub fn report(e: Vec<MqlError<'q>>) {
        unimplemented!()
    }
}

#[test]
fn tokenize_ok() {
    let toks = |q: &'static str| tokenize(q).unwrap().into_iter().map(|t| t.ty).collect_vec();
    use TokenType::*;

    assert_eq!(toks("artist == Björk"),           [Lit("artist"), BinOp("=="), Lit("Björk")]);
    assert_eq!(toks("artist is not 米津玄師"),      [Lit("artist"), BinOp("is not"), Lit("米津玄師")]);
    assert_eq!(toks("a == 'Сплин и др.'"),         [Lit("a"), BinOp("=="), QuotedLit("Сплин и др.")]);
    assert_eq!(toks("orłowski == a"),              [Lit("orłowski"), BinOp("=="), Lit("a")]);
    assert_eq!(toks("!(a.len >= 2)"),              [UnOp("!"), OParen, Lit("a"), BinOp("."), Lit("len"), BinOp(">="), Number(2), CParen]);
    assert_eq!(toks("a\u{3000}!=\u{a0}b"),         [Lit("a"), BinOp("!="), Lit("b")]);
    assert_eq!(toks("a == 2pac || a == 50cent"),   [Lit("a"), BinOp("=="), Lit("2pac"), BinOp("||"), Lit("a"), BinOp("=="), Lit("50cent")]);

    // Sirynka: This one is not going to pass `parse` but is fine as tok test
    assert_eq!(toks("1 < a.len"),                  [Number(1), BinOp("<"), Lit("a"), BinOp("."), Lit("len")]);

    assert_eq!(toks("a == 4294967296x"),           [Lit("a"), BinOp("=="), Lit("4294967296x")]);
}

#[test]
fn tokenize_apostrophe() {
    // Claude: errors give the query from the error onward, a lone `'` wouldn't say which quote it is
    let toks = |q: &'static str| match tokenize(q) {
        Ok(t) => Ok(t.into_iter().map(|t| t.ty).collect_vec()),
        Err(e) => Err((&q[e.range.start..], e.reason.into_owned())),
    };
    use TokenType::*;
    let glued = |s: &'static str| Err((s, "`'` inside a value, wrap the whole value in double quotes".to_string()));

    assert_eq!(toks("a == \"5'nizza\""),            Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5'nizza")]));
    assert_eq!(toks("a == '5\"nizza'"),             Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5\"nizza")]));
    assert_eq!(toks("a == \"Guns N' Roses\""),      Ok(vec![Lit("a"), BinOp("=="), QuotedLit("Guns N' Roses")]));
    assert_eq!(toks("a=='x'"),                      Ok(vec![Lit("a"), BinOp("=="), QuotedLit("x")]));

    assert_eq!(toks("a == 5'nizza"),                glued("'nizza"));
    assert_eq!(toks("a == 5'nizza && b == 'x'"),    glued("'nizza && b == 'x'"));
    assert_eq!(toks("a == '5'nizza'"),              glued("'nizza'"));
    assert_eq!(toks("a == 'it's'"),                 glued("'s'"));
    assert_eq!(toks("a == ' a'int nobody'"),        glued("'int nobody'"));
    assert_eq!(toks("a == ''x"),                    glued("'x"));
    assert_eq!(toks("a == '5\"nizza\"'"),           Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5\"nizza\"")]));
    assert_eq!(toks("a == \"5\"nizza\""),           Err(("\"nizza\"", "`\"` inside a value, wrap the whole value in single quotes".to_string())));

    // Claude: the `N'` closes fine (a space follows), only the last quote is glued
    assert_eq!(toks("a == 'Guns N' Roses'"),        glued("'"));

    // Claude: nobody glues quotes to keywords
    assert_eq!(toks("a == 'x'and b == c"),          glued("'and b == c"));
    assert_eq!(toks("a is'x'"),                     glued("'x'"));

    // Claude: curly apostrophe is neither a quote nor a word char
    assert_eq!(toks("a == 5’nizza"),                Err(("’nizza", "unexpected token".to_string())));
    assert_eq!(toks("a == '5’nizza'"),              Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5’nizza")]));
}

#[test]
fn tokenize_err() {
    let err = |q: &'static str| {
        let e = tokenize(q).unwrap_err();
        (&q[e.range.start..e.range.end], e.reason.into_owned())
    };

    assert_eq!(err("a == 🎵"),       ("🎵",   "unexpected token".into()));
    assert_eq!(err("a == 'Björk"),  ("'Björk", "unterminated quote".into()));
    assert_eq!(MqlError { query: "é == ✗", range: 6..9, reason: "x".into() }.to_string(), "\n'é == ✗'\n      ^\n      x\n");
}
