use std::{borrow::Cow, fmt::Display, ops::Range};
use itertools::Itertools;
use thiserror::Error;
use crate::song::Song;

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
    pub ranges: Vec<Range<usize>>,
    pub reason: Cow<'q, str>,
}

impl<'q> MqlError<'q> {
    pub fn new(query: &'q str, range: Range<usize>, reason: impl Into<Cow<'q, str>>) -> Self {
        MqlError { query, ranges: vec![range], reason: reason.into() }
    }
}

impl Display for MqlError<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Claude: `ranges` are in bytes, the terminal column is in chars. `start` is the line's offset in the query.
        // Claude: no carets under indentation, a range crossing lines (unclosed paren) would underline it
        let carets_for = |start: usize, line: &str| -> String {
            let text = line.len() - line.trim_start().len()..line.trim_end().len();
            let carets: String = line.char_indices()
                .map(|(i, _)| if text.contains(&i) && self.ranges.iter().any(|r| r.contains(&(start + i))) { '^' } else { ' ' })
                .collect();
            carets.trim_end().to_string()
        };
        let indent = |carets: &str| " ".repeat(carets.len() - carets.trim_start().len());

        writeln!(f)?;
        if !self.query.contains('\n') {
            let carets = carets_for(0, self.query);
            writeln!(f, "'{}'", self.query)?;
            writeln!(f, " {carets}")?;
            writeln!(f, " {}{}", indent(&carets), self.reason)?;
            return Ok(());
        }

        let mut offset = 0;
        let lines = self.query.split('\n').map(|line| { let start = offset; offset += line.len() + 1; (start, line) }).collect_vec();
        let last = lines.iter().rposition(|&(start, line)| !carets_for(start, line).is_empty()).unwrap_or(0);
        let w = lines.len().to_string().len();

        for (n, &(start, line)) in lines.iter().enumerate() {
            writeln!(f, "{:>w$} | {line}", n + 1)?;
            let carets = carets_for(start, line);
            if !carets.is_empty() { writeln!(f, "{:w$} | {carets}", "")?; }
            if n == last { writeln!(f, "{:w$} | {}{}", "", indent(&carets), self.reason)?; }
        }
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
const CMP_STR_OPS: &[&str] = &["contains", "not contains", "does not contain"];

const BOOL_UNOPS: &[&str] = &["!", "not"];
const BOOL_BINOPS: &[&str] = &["||", "or", "&&", "and"];

const TAG_FIELDS_OF_NUM_TYPE: &[&str] = &["len"];
const TAG_FIELDS_OF_STR_TYPE: &[&str] = &["str"];

// Location
#[derive(Debug)]
pub struct Loc<Expr> {
    expr: Expr,
    at: Range<usize>,
}

#[derive(Debug)]
pub struct Val<'q> {
    str: &'q str,
    quoted: bool,
}

#[derive(Debug)]
pub enum Expr<'q> {
    BinOp {
        op: &'q str, // BOOL_BINOPS
        lhs_and_rhs: Box<(Loc<Expr<'q>>, Loc<Expr<'q>>)>,
    },
    UnOp {
        op: &'q str, // BOOL_UNOPS
        expr: Box<Loc<Expr<'q>>>,
    },
    Set {
        tag: Loc<&'q str>,
        op: Loc<&'q str>, // CMP_SET_OPS
        val: Loc<Val<'q>>,
    },
    Num {
        tag: Loc<&'q str>,
        member: Loc<&'q str>, // TAG_FIELDS_OF_NUM_TYPE
        op: Loc<&'q str>,     // CMP_NUM_OPS or CMP_SET_OPS
        num: Loc<Num>,
    },
    Str {
        tag: Loc<&'q str>,
        member: Loc<&'q str>, // TAG_FIELDS_OF_STR_TYPE
        op: Loc<&'q str>,     // CMP_STR_OPS or CMP_SET_OPS
        val: Loc<Val<'q>>,
    },
    Bool(bool),
}

// Claude: `_` for tag names like `album_artist`, `musicbrainz_trackid`
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn validate_quotes<'q>(q: &'q str) -> Result<(), MqlError<'q>> {
    let is_word = |c: Option<char>| c.is_some_and(is_word_char);
    let glued = |at: usize, quote: char| {
        let other = if quote == '\'' { "double" } else { "single" };
        let reason = format!("`{quote}` inside a value, wrap the whole value in {other} quotes");
        MqlError::new(q, at..at+1, reason)
    };

    // Claude: a quote glued to a letter or digit is part of a value (`5'nizza`, `'it's'`), not a delimiter.
    let mut open = None;
    let mut quotes = Vec::new(); // (at, quote, glued)
    for (i, c) in q.char_indices() {
        match (open, c) {
            (None, '\'' | '"') => {
                open = Some(c);
                quotes.push((i, c, is_word(q[..i].chars().next_back())));
            },
            (Some(quote), c) if c == quote => {
                open = None;
                quotes.push((i, c, is_word(q[i+1..].chars().next())));
            },
            _ => {},
        }
    }

    // Claude: a glued odd quote is an apostrophe in a value (`Guns N' Roses`), none glued is a missing closing quote (`'Björk`)
    if let Some(odd) = open {
        let ranges = quotes.iter().filter(|&&(_, c, _)| c == odd).map(|&(i, _, _)| i..i+1).collect();
        let other = if odd == '\'' { '"' } else { '\'' };
        let reason = match quotes.iter().any(|&(_, c, glued)| c == odd && glued) {
            true => format!("odd number of quotes, perhaps it should be wrapped in `{other}`"),
            false => "odd number of quotes".to_string(),
        };
        return Err(MqlError { query: q, ranges, reason: reason.into() });
    }

    match quotes.iter().find(|&&(_, _, glued)| glued) {
        Some(&(i, c, _)) => Err(glued(i, c)),
        None => Ok(()),
    }
}

fn tokenize<'q>(q: &'q str) -> Result<Vec<Token<'q>>, MqlError<'q>> {
    validate_quotes(q)?;

    // Claude: multi-word ops first, so `is not` isn't read as `is`
    let bin_ops = [BOOL_BINOPS, CMP_SET_OPS, CMP_NUM_OPS, CMP_STR_OPS, &["."]]
        .iter().copied().flatten().sorted_by_key(|op| !op.contains(' ')).collect_vec();

    let consume = |c: usize| -> Result<(Token<'q>, usize), MqlError<'q>> {
        // Claude: `or` must not match the start of `order`
        let matches_word = |c: usize, op: &str| {
            let is_word = op.starts_with(|c: char| c.is_alphabetic());
            q[c..].starts_with(op) && !(is_word && q[c+op.len()..].starts_with(is_word_char))
        };

        // Claude: words of a multi-word op (`is not`) can be separated by any whitespace
        let match_op = |c: usize, op: &str| -> Option<usize> {
            let mut e = c;
            for word in op.split(' ') {
                e += q[e..].len() - q[e..].trim_start().len();
                if !matches_word(e, word) { return None; }
                e += word.len();
            }
            Some(e)
        };

        let tok = |ty, e: usize| Ok((Token { ty, str: &q[c..e] }, e));

        if q[c..].starts_with('(') { return tok(TokenType::OParen, c+1); }
        if q[c..].starts_with(')') { return tok(TokenType::CParen, c+1); }

        if let Some((op, e)) = bin_ops   .iter().find_map(|op| match_op(c, op).map(|e| (op, e))) { return tok(TokenType::BinOp(op), e); }
        if let Some((op, e)) = BOOL_UNOPS.iter().find_map(|op| match_op(c, op).map(|e| (op, e))) { return tok(TokenType::UnOp(op), e); }

        if let Some(quote @ ('\'' | '"')) = q[c..].chars().next() {
            let Some(e) = q[c + 1..].find(quote).and_then(|e| Some(e + c + 1)) else { panic!("validate_quotes has a bug"); };
            return tok(TokenType::QuotedLit(&q[c + 1..e]), e + 1);
        }

        // Claude: so `2pac` and `50cent` are lits
        if q[c..].starts_with(is_word_char) {
            let e = q[c..].find(|c: char| !is_word_char(c)).unwrap_or(q.len() - c) + c;
            let word = &q[c..e];
            let is_number = word.starts_with(|c: char| c.is_ascii_digit()) && word.bytes().all(|b| b.is_ascii_digit() || b == b'_');
            if !is_number { return tok(TokenType::Lit(word), e); }

            // Claude: `1_000` is 1000
            let n = word.bytes().filter(|&b| b != b'_').try_fold(0 as Num, |n, b| n.checked_mul(10)?.checked_add((b - b'0') as Num));
            let Some(n) = n else { return Err(MqlError::new(q, c..e, format!("number too large, max is {}", Num::MAX))); };
            return tok(TokenType::Number(n), e);
        }

        let len = q[c..].chars().next().map_or(1, char::len_utf8);
        Err(MqlError::new(q, c..c+len, "unexpected token"))
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

struct Parser;
impl<'q, 't> Parser {
    fn error_at_toks(q: &'q str, toks: &'t [Token<'q>], range: Range<usize>, reason: impl Into<Cow<'q, str>>) -> MqlError<'q> {
        MqlError::new(q, Self::toks_to_bytes(q, toks, range), reason)
    }

    fn toks_to_bytes(q: &'q str, toks: &'t [Token<'q>], range: Range<usize>) -> Range<usize> {
        let first = &toks[range.start];
        let last = &toks[range.end-1];
        let start = first.str.as_ptr() as usize - q.as_ptr() as usize;
        let end = last.str.as_ptr() as usize - q.as_ptr() as usize + last.str.len();
        start..end
    }

    fn validate_parens(q: &'q str, toks: &'t [Token<'q>]) -> Result<(), MqlError<'q>> {
        use tok::*;

        let mut n = 0;
        let mut last_balanced_idx = 0;
        for (i, t) in toks.iter().enumerate() {
            match t {
                oparen!() => n += 1,
                cparen!() => n -= 1,
                _ => {},
            }

            match n {
                ..0 => return Err(Self::error_at_toks(q, toks, 0..i+1, "unmatched paren")),
                0 => last_balanced_idx = i+1,
                _ => {},
            }
        }

        if n > 0 { return Err(Self::error_at_toks(q, toks, last_balanced_idx..toks.len(), "unclosed paren")); }
        Ok(())
    }

    fn skip_paren_group(toks: &'t [Token<'q>], range: Range<usize>) -> usize {
        let mut n = 1;
        let mut i = range.start;
        while n > 0 && i < range.end {
            match toks[i].ty {
                TokenType::OParen => n += 1,
                TokenType::CParen => n -= 1,
                _ => {},
            }

            i += 1;
        }

        assert!(n == 0, "We're fucked: {:#?}", &toks[range.start..i]);

        i
    }

    fn find_bool_binop_with_min_prec(toks: &'t [Token<'q>], range: Range<usize>) -> Option<usize> {
        use tok::*;

        const { assert!(BOOL_BINOPS.len() == 4) };
        let prec = |op: &str| match op {
            "||" | "or" => 0,
            "&&" | "and" => 1,
            _ => unreachable!(),
        };

        let mut min_i = None;
        let mut min_prec = usize::MAX;
        let mut i = range.start;
        while i < range.end {
            match toks[i] {
                oparen!() => i = Self::skip_paren_group(toks, i+1..range.end),

                binop!(op)
                    if BOOL_BINOPS.contains(&op)
                    && prec(op) <= min_prec
                => {
                    min_prec = prec(op);
                    min_i = Some(i);
                    i += 1;
                },

                _ => i += 1,
            }
        }

        min_i
    }

    fn parse_unop(q: &'q str, toks: &'t [Token<'q>], range: Range<usize>) -> Result<Loc<Expr<'q>>, MqlError<'q>> {
        use tok::*;

        let unop!(op) = toks[range.start] else { unreachable!() };
        let operand = range.start+1..range.end;
        if operand.is_empty() { return Err(Self::error_at_toks(q, toks, range, format!("expected a condition after `{op}`"))); }

        let inner = Self::parse_expr(q, toks, operand)?;
        Ok(Loc { at: Self::toks_to_bytes(q, toks, range), expr: Expr::UnOp { op, expr: Box::new(inner) } })
    }

    fn parse_group(q: &'q str, toks: &'t [Token<'q>], range: Range<usize>) -> Result<Loc<Expr<'q>>, MqlError<'q>> {
        use tok::*;

        let end = Self::skip_paren_group(toks, range.start+1..range.end);
        let inside_parens = range.start+1..end-1;
        if inside_parens.is_empty() { return Err(Self::error_at_toks(q, toks, range.start..end, "what do you want me to do with it?")); }

        match end {
            end
                if end < range.end
                && matches!(toks[end], oparen!() | unop!(_) | lit!(_))
                => Err(Self::error_at_toks(q, toks, end-1..end+1, format!("expected one of {BOOL_BINOPS:?}"))),

            end if end < range.end
                => Err(Self::error_at_toks(q, toks, end..range.end, format!("expected one of {BOOL_BINOPS:?} followed by a condition"))),

            _ => {
                let inner = Self::parse_expr(q, toks, inside_parens)?;
                Ok(Loc { at: Self::toks_to_bytes(q, toks, range), expr: inner.expr })
            }
        }
    }

    fn parse_cmp_binop(q: &'q str, toks: &'t [Token<'q>], range: Range<usize>) -> Result<Loc<Expr<'q>>, MqlError<'q>> {
        use tok::*;

        let is_cmp_op = |t: &Token| matches!(t, binop!(o) if CMP_SET_OPS.contains(o) || CMP_NUM_OPS.contains(o) || CMP_STR_OPS.contains(o));
        let Some(i) = (range.start..range.end).find(|&i| is_cmp_op(&toks[i])) else {
            // Claude: we know what's on the left, so list the ops it takes (`genre.str does no contain rap`)
            let (lhs_len, ops) = match toks[range.start..range.end] {
                [lit!(_), binop!("."), lit!(field), _, ..] if TAG_FIELDS_OF_STR_TYPE.contains(&field) => (3, [CMP_STR_OPS, CMP_SET_OPS].concat()),
                [lit!(_), binop!("."), lit!(field), _, ..] if TAG_FIELDS_OF_NUM_TYPE.contains(&field) => (3, [CMP_NUM_OPS, CMP_SET_OPS].concat()),
                [lit!(_), ref next, ..] if !matches!(next, binop!(".")) => (1, CMP_SET_OPS.to_vec()),
                _ => return Err(Self::error_at_toks(q, toks, range, "expected a comparison, like `tagname == value`")),
            };
            return Err(Self::error_at_toks(q, toks, range.start+lhs_len..range.end, format!("expected one of {ops:?}")));
        };

        let binop!(op) = toks[i] else { unreachable!() };
        let lhs = range.start..i;
        let rhs = i+1..range.end;
        let loc = |r: Range<usize>| Self::toks_to_bytes(q, toks, r);
        if lhs.is_empty() { return Err(Self::error_at_toks(q, toks, i..i+1, "expected a tag name")); }
        if rhs.is_empty() { return Err(Self::error_at_toks(q, toks, i..i+1, "nothing to compare to")); }

        let parse_val = || match toks[rhs.start..rhs.end] {
            [lit!(v)]    => Ok(Val { str: v, quoted: false }),
            [qlit!(v)]   => Ok(Val { str: v, quoted: true }),
            [number!(_)] => Ok(Val { str: toks[rhs.start].str, quoted: false }),
            ref words if words.iter().all(|t| matches!(t, lit!(_) | number!(_)))
                => Err(Self::error_at_toks(q, toks, rhs.start..rhs.end,
                    format!("values with spaces have to be quoted, like `'{}'`", &q[loc(rhs.start..rhs.end)])
                )),
            _ => Err(Self::error_at_toks(q, toks, rhs.start..rhs.end, "right side has to be lit")),
        };

        let type_mismatch = |needs: &str, is: &str| Self::error_at_toks(q, toks, lhs.start..i+1,
            format!("`{op}` needs {needs}, `{}` is {is}", &q[loc(lhs.start..lhs.end)])
        );

        let expr = match toks[lhs.start..lhs.end] {
            [lit!(tag)] => {
                if CMP_NUM_OPS.contains(&op) { return Err(type_mismatch("a number", "a list")); }
                if CMP_STR_OPS.contains(&op) { return Err(type_mismatch("a string", "a list")); }
                let val = parse_val()?;

                Expr::Set {
                    tag: Loc { expr: tag, at: loc(lhs) },
                    op: Loc { expr: op, at: loc(i..i+1) },
                    val: Loc { expr: val, at: loc(rhs) },
                }
            }

            [lit!(tag), binop!("."), lit!(member)]
                if TAG_FIELDS_OF_NUM_TYPE.contains(&member)
                => {
                if CMP_STR_OPS.contains(&op) { return Err(type_mismatch("a string", "a number")); }
                let num = match toks[rhs.start..rhs.end] {
                    [number!(n)] => n,
                    _ => return Err(Self::error_at_toks(q, toks, rhs, "right side has to be a number")),
                };

                Expr::Num {
                    tag: Loc { expr: tag, at: loc(lhs.start..lhs.start+1) },
                    member: Loc { expr: member, at: loc(lhs.end-1..lhs.end) },
                    op: Loc { expr: op, at: loc(i..i+1) },
                    num: Loc { expr: num, at: loc(rhs) },
                }
            }

            [lit!(tag), binop!("."), lit!(member)]
                if TAG_FIELDS_OF_STR_TYPE.contains(&member)
                => {
                if CMP_NUM_OPS.contains(&op) { return Err(type_mismatch("a number", "a string")); }
                let val = parse_val()?;

                Expr::Str {
                    tag: Loc { expr: tag, at: loc(lhs.start..lhs.start+1) },
                    member: Loc { expr: member, at: loc(lhs.end-1..lhs.end) },
                    op: Loc { expr: op, at: loc(i..i+1) },
                    val: Loc { expr: val, at: loc(rhs) },
                }
            }

            [lit!(tag), binop!("."), lit!(_)] => return Err(Self::error_at_toks(q, toks, lhs.end-1..lhs.end, format!("`{tag}` only has {:?}", [TAG_FIELDS_OF_NUM_TYPE, TAG_FIELDS_OF_STR_TYPE].concat()))),
            _ if CMP_SET_OPS.contains(&op)    => return Err(Self::error_at_toks(q, toks, lhs, "only a tag name can be on the left")),
            [number!(_), ..]                  => return Err(Self::error_at_toks(q, toks, lhs, "consts should be on the right side of the expression")),
            _ if CMP_STR_OPS.contains(&op)    => return Err(Self::error_at_toks(q, toks, lhs, "expected a tag field, like `artist.str`")),
            _                                 => return Err(Self::error_at_toks(q, toks, lhs, "expected a tag field, like `artist.len`")),
        };

        Ok(Loc { at: loc(range), expr })
    }

    fn parse_expr(q: &'q str, toks: &'t [Token<'q>], range: Range<usize>) -> Result<Loc<Expr<'q>>, MqlError<'q>> {
        use tok::*;

        let Some(i) = Self::find_bool_binop_with_min_prec(toks, range.start..range.end) else {
            return match toks[range.start..range.end] {
                [unop!(_), ..] => Self::parse_unop(q, toks, range),
                [oparen!(), ..] => Self::parse_group(q, toks, range),
                _ => Self::parse_cmp_binop(q, toks, range),
            };
        };

        let binop!(op) = toks[i] else { unreachable!() };

        // Sirynka: This looks meh
        let is_bool_binop = |i: usize| matches!(toks[i], binop!(o) if BOOL_BINOPS.contains(&o));
        if i > range.start && is_bool_binop(i-1) { return Err(Self::error_at_toks(q, toks, i-1..i+1, "two logical ops in a row")); }
        if i+1 < range.end && is_bool_binop(i+1) { return Err(Self::error_at_toks(q, toks, i..i+2, "two logical ops in a row")); }
        if i == range.start { return Err(Self::error_at_toks(q, toks, i..i+1, format!("expected a condition before `{op}`"))); }
        if i+1 == range.end { return Err(Self::error_at_toks(q, toks, i..i+1, format!("expected a condition after `{op}`"))); }

        // Claude: `artist == simon and garfunkel` is one value with spaces, not two conditions
        if matches!(op, "and" | "or") {
            let is_word = |t: &Token| matches!(t, lit!(_) | number!(_));
            let start = i - toks[range.start..i].iter().rev().take_while(|t| is_word(t)).count();
            let end = i+1 + toks[i+1..range.end].iter().take_while(|t| is_word(t)).count();

            let after_value_op = start > range.start && matches!(toks[start-1], binop!(o) if CMP_SET_OPS.contains(&o) || CMP_STR_OPS.contains(&o));
            let before_logical_op = end == range.end || is_bool_binop(end);

            if start < i && end > i+1 && after_value_op && before_logical_op {
                return Err(Self::error_at_toks(q, toks, start..end, format!(
                    "values with spaces have to be quoted, like `'{}'`",
                    &q[Self::toks_to_bytes(q, toks, start..end)]
                )));
            }
        }

        let lhs = Self::parse_expr(q, toks, range.start..i)?;
        let rhs = Self::parse_expr(q, toks, i+1..range.end)?;

        Ok(Loc {
            at: lhs.at.start..rhs.at.end,
            expr: Expr::BinOp { op, lhs_and_rhs: Box::new((lhs, rhs)) },
        })
    }

    fn parse(q: &'q str, toks: &'t [Token<'q>]) -> Result<Loc<Expr<'q>>, MqlError<'q>> {
        if toks.is_empty() { return Ok(Loc { expr: Expr::Bool(false), at: 0..q.len() }); }
        Self::validate_parens(q, toks)?;
        Self::parse_expr(q, toks, 0..toks.len())
    }
}

pub fn eval(q: &Loc<Expr>, song: &Song) -> bool {
    match &q.expr {
        Expr::BinOp { op, lhs_and_rhs } => {
            let (lhs, rhs) = lhs_and_rhs.as_ref();

            match *op {
                "&&" | "and" => eval(lhs, song) && eval(rhs, song),
                "||" | "or" => eval(lhs, song) || eval(rhs, song),
                _ => unreachable!(),
            }
        },

        Expr::UnOp { op, expr } => match *op {
            "!" | "not" => !eval(expr, song),
            _ => unreachable!(),
        },

        Expr::Set { tag, op, val } => {
            let val = val.expr.str.to_lowercase();
            let eq = song.tag(tag.expr).any(|v| v.to_lowercase() == val);

            match op.expr {
                "==" | "is" => eq,
                "!=" | "is not" => !eq,
                _ => unreachable!(),
            }
        },

        Expr::Num { tag, member, op, num } => {
            let lhs = match member.expr {
                "len" => song.tag(tag.expr).count() as Num,
                _ => unreachable!(),
            };

            let rhs = num.expr;

            match op.expr {
                "==" | "is" => lhs == rhs,
                "!=" | "is not" => lhs != rhs,
                ">" => lhs > rhs,
                "<" => lhs < rhs,
                ">=" => lhs >= rhs,
                "<=" => lhs <= rhs,
                _ => unreachable!(),
            }
        },

        Expr::Str { tag, member, op, val } => {
            let val = val.expr.str.to_lowercase();

            let mut values = match member.expr {
                "str" => song.tag(tag.expr).map(str::to_lowercase),
                _ => unreachable!(),
            };

            match op.expr {
                "contains" => values.any(|v| v.contains(&val)),
                "not contains" | "does not contain" => !values.any(|v| v.contains(&val)),
                "==" | "is" => values.any(|v| v == val),
                "!=" | "is not" => !values.any(|v| v == val),
                _ => unreachable!(),
            }
        },

        Expr::Bool(b) => *b,
    }
}

pub struct MQL;
impl<'q, 't> MQL {
    pub fn parse(q: &'q str) -> Result<Loc<Expr<'q>>, MqlError<'q>> {
        let toks = tokenize(q)?;
        let ast = Parser::parse(q, &toks)?;
        Ok(ast)
    }
}

impl<T: Display> Display for Loc<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.expr)
    }
}

impl Display for Val<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.quoted, self.str.contains('\'')) {
            (false, _) => write!(f, "{}", self.str),
            (true, false) => write!(f, "'{}'", self.str),
            (true, true) => write!(f, "\"{}\"", self.str),
        }
    }
}

impl Display for Expr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::BinOp { op, lhs_and_rhs } => write!(f, "({} {op} {})", lhs_and_rhs.0, lhs_and_rhs.1),
            Expr::UnOp { op, expr } => write!(f, "({op} {expr})"),
            Expr::Set { tag, op, val } => write!(f, "({tag} {op} {val})"),
            Expr::Num { tag, member, op, num } => write!(f, "({tag}.{member} {op} {num})"),
            Expr::Str { tag, member, op, val } => write!(f, "({tag}.{member} {op} {val})"),
            Expr::Bool(b) => write!(f, "{b}"),
        }
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
    assert_eq!(toks("g.str not  contains rap"),    [Lit("g"), BinOp("."), Lit("str"), BinOp("not contains"), Lit("rap")]);
    assert_eq!(toks("a is notable"),               [Lit("a"), BinOp("is"), Lit("notable")]);
    assert_eq!(toks("album_artist == ac_dc"),      [Lit("album_artist"), BinOp("=="), Lit("ac_dc")]);
    assert_eq!(toks("a == b or_c"),                [Lit("a"), BinOp("=="), Lit("b"), Lit("or_c")]);
    assert_eq!(toks("a == 2pac || a == 50cent"),   [Lit("a"), BinOp("=="), Lit("2pac"), BinOp("||"), Lit("a"), BinOp("=="), Lit("50cent")]);

    // Sirynka: This one is not going to pass `parse` but is fine as tok test
    assert_eq!(toks("1 < a.len"),                  [Number(1), BinOp("<"), Lit("a"), BinOp("."), Lit("len")]);

    assert_eq!(toks("a == 4294967296x"),           [Lit("a"), BinOp("=="), Lit("4294967296x")]);
    assert_eq!(toks("a.len > 1_000"),              [Lit("a"), BinOp("."), Lit("len"), BinOp(">"), Number(1000)]);
    assert_eq!(toks("a == 1_000x"),                [Lit("a"), BinOp("=="), Lit("1_000x")]);
    assert_eq!(toks("a == _1"),                    [Lit("a"), BinOp("=="), Lit("_1")]);
    assert_eq!(toks("a.len > 4_294_967_295"),      [Lit("a"), BinOp("."), Lit("len"), BinOp(">"), Number(4294967295)]);
}

#[test]
fn tokenize_apostrophe() {
    let toks = |q: &'static str| match tokenize(q) {
        Ok(t) => Ok(t.into_iter().map(|t| t.ty).collect_vec()),
        Err(e) => Err((&q[e.ranges[0].start..], e.reason.into_owned())),
    };
    use TokenType::*;
    let glued = |s: &'static str| Err((s, "`'` inside a value, wrap the whole value in double quotes".to_string()));

    assert_eq!(toks("a == \"5'nizza\""),            Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5'nizza")]));
    assert_eq!(toks("a == '5\"nizza'"),             Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5\"nizza")]));
    assert_eq!(toks("a == \"Guns N' Roses\""),      Ok(vec![Lit("a"), BinOp("=="), QuotedLit("Guns N' Roses")]));
    assert_eq!(toks("a=='x'"),                      Ok(vec![Lit("a"), BinOp("=="), QuotedLit("x")]));

    assert_eq!(toks("a == ''x"),                    glued("'x"));
    assert_eq!(toks("a == '5\"nizza\"'"),           Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5\"nizza\"")]));
    assert_eq!(toks("a == 'Rock 'n' Roll'"),        glued("'n' Roll'"));
    assert_eq!(toks("a == \"5\"ni\"zza\""),          Err(("\"ni\"zza\"", "`\"` inside a value, wrap the whole value in single quotes".to_string())));

    assert_eq!(toks("a == 'x'and b == c"),          glued("'and b == c"));
    assert_eq!(toks("a == 'x'_y"),                  glued("'_y"));
    assert_eq!(toks("a is'x'"),                     glued("'x'"));

    // Claude: curly apostrophe is neither a quote nor a word char
    assert_eq!(toks("a == 5’nizza"),                Err(("’nizza", "unexpected token".to_string())));
    assert_eq!(toks("a == '5’nizza'"),              Ok(vec![Lit("a"), BinOp("=="), QuotedLit("5’nizza")]));
}

#[test]
fn tokenize_odd_quotes() {
    const ODD: &str = "odd number of quotes";
    const WRAP_IN_DOUBLE: &str = "odd number of quotes, perhaps it should be wrapped in `\"`";
    const WRAP_IN_SINGLE: &str = "odd number of quotes, perhaps it should be wrapped in `'`";

    check(r#"a == 'Björk"#,
          r#"     ^"#, ODD);
    check(r#"a == 5'nizza"#,
          r#"      ^"#, WRAP_IN_DOUBLE);
    check(r#"a == 'it's'"#,
          r#"     ^  ^ ^"#, WRAP_IN_DOUBLE);
    check(r#"a == Guns N' Roses"#,
          r#"           ^"#, WRAP_IN_DOUBLE);
    check(r#"a == 'Guns N' Roses'"#,
          r#"     ^      ^      ^"#, WRAP_IN_DOUBLE);
    check(r#"a == 'x' || b == 'y"#,
          r#"     ^ ^         ^"#, ODD);
    check(r#"a == 5'nizza && b == 'x'"#,
          r#"      ^              ^ ^"#, WRAP_IN_DOUBLE);
    check(r#"a == "it's"#,
          r#"     ^"#, ODD);
    check(r#"a == "5"nizza""#,
          r#"     ^ ^     ^"#, WRAP_IN_SINGLE);

    #[track_caller]
    fn check(q: &str, expected: &str, reason: &str) {
        let e = tokenize(q).unwrap_err();
        assert_eq!(e.reason, reason);

        let shown = e.to_string();
        let marks = &shown.lines().nth(2).unwrap()[1..];
        let w = q.chars().count();
        assert!(marks == expected, "\n{q}\n{expected:<w$} <- expected\n{marks:<w$} <- got\n");
    }
}

#[test]
fn multiline_err() {
    let err = |q: &'static str| Parser::parse(q, &tokenize(q).unwrap()).unwrap_err().to_string();

    assert_eq!(err("artist == eminem\n  && genre.str contians rap"), [
        "",
        "1 | artist == eminem",
        "2 |   && genre.str contians rap",
        "  |                ^^^^^^^^^^^^",
        &format!("  |                expected one of {:?}", [CMP_STR_OPS, CMP_SET_OPS].concat()),
        "",
    ].join("\n"));

    assert_eq!(err("(a == b\n  || c == d\n"), [
        "",
        "1 | (a == b",
        "  | ^^^^^^^",
        "2 |   || c == d",
        "  |   ^^^^^^^^^",
        "  |   unclosed paren",
        "3 | ",
        "",
    ].join("\n"));
}

#[test]
fn tokenize_err() {
    let err = |q: &'static str| {
        let e = tokenize(q).unwrap_err();
        (&q[e.ranges[0].start..e.ranges[0].end], e.reason.into_owned())
    };

    assert_eq!(err("a.len > 4294967296"), ("4294967296", "number too large, max is 4294967295".into()));
    assert_eq!(err("a == 🎵"),       ("🎵",   "unexpected token".into()));
    assert_eq!(MqlError::new("é == ✗", 6..9, "x").to_string(), "\n'é == ✗'\n      ^\n      x\n");
}

#[test]
fn skip_paren_group() {
    let q = "a == b || (c == d && e == f) && g == h";
    let toks = &tokenize(q).unwrap();
    let i = Parser::skip_paren_group(toks, 5..toks.len());
    assert_eq!(toks[i].ty, TokenType::BinOp("&&"));
}

#[test]
fn find_bool_binop_with_min_prec() {
    let find = |q: &'static str| {
        let toks = tokenize(q).unwrap();
        Parser::find_bool_binop_with_min_prec(&toks, 0..toks.len()).map(|i| toks[i..].iter().map(|t| t.str).join(" "))
    };

    assert_eq!(find("a == b || c == d || e == f"),      Some("|| e == f".into()));
    assert_eq!(find("a == b && c == d || e == f"),      Some("|| e == f".into()));
    assert_eq!(find("a == b || c == d && e == f"),      Some("|| c == d && e == f".into()));
    assert_eq!(find("a == b and c == d or e == f"),     Some("or e == f".into()));
    assert_eq!(find("a == b || c == d or e == f"),      Some("or e == f".into()));
    assert_eq!(find("(a == b || c == d) && e == f"),    Some("&& e == f".into()));
    assert_eq!(find("!(a == b || c == d)"),             None);
    assert_eq!(find("a == b"),                          None);
}

#[test]
fn parse_cmp_binop_err() {
    let err = |q: &'static str| {
        let toks = tokenize(q).unwrap();
        let e = Parser::parse(q, &toks).unwrap_err();
        (&q[e.ranges[0].start..e.ranges[0].end], e.reason.into_owned())
    };
    let s = |s: &str| s.to_string();

    assert_eq!(err("a"),                          ("a",               s("expected a comparison, like `tagname == value`")));
    assert_eq!(err("a.len"),                      ("a.len",           s("expected a comparison, like `tagname == value`")));
    assert_eq!(err("1 == a"),                     ("1",               s("only a tag name can be on the left")));
    assert_eq!(err("a.len == b"),                 ("b",               s("right side has to be a number")));
    assert_eq!(err("a.foo == 1"),                 ("foo",             s("`a` only has [\"len\", \"str\"]")));
    assert_eq!(err("== a"),                       ("==",              s("expected a tag name")));
    assert_eq!(err("a =="),                       ("==",              s("nothing to compare to")));
    assert_eq!(err("a == ed sheeran"),            ("ed sheeran",      s("values with spaces have to be quoted, like `'ed sheeran'`")));
    assert_eq!(err("a is not the 2 of us"),       ("the 2 of us",     s("values with spaces have to be quoted, like `'the 2 of us'`")));
    assert_eq!(err("a == b.len"),                 ("b.len",           s("right side has to be lit")));
    assert_eq!(err("a > 1"),                      ("a >",             s("`>` needs a number, `a` is a list")));
    assert_eq!(err("1 < a.len"),                  ("1",               s("consts should be on the right side of the expression")));
    assert_eq!(err("a.foo > 1"),                  ("foo",             s("`a` only has [\"len\", \"str\"]")));
    assert_eq!(err("genre contains rap"),         ("genre contains",     s("`contains` needs a string, `genre` is a list")));
    assert_eq!(err("genre.str > 1"),              ("genre.str >",        s("`>` needs a number, `genre.str` is a string")));
    assert_eq!(err("genre.len contains x"),       ("genre.len contains", s("`contains` needs a string, `genre.len` is a number")));
    assert_eq!(err("genre.len not  contains x"),  ("genre.len not  contains", s("`not contains` needs a string, `genre.len` is a number")));
    assert_eq!(err("genre.str contains hip hop"), ("hip hop",         s("values with spaces have to be quoted, like `'hip hop'`")));
    assert_eq!(err("genre.str does no contain rap"), ("does no contain rap", format!("expected one of {:?}", [CMP_STR_OPS, CMP_SET_OPS].concat())));
    assert_eq!(err("a.len 1"),                  ("1",               format!("expected one of {:?}", [CMP_NUM_OPS, CMP_SET_OPS].concat())));
    assert_eq!(err("a.len >"),                    (">",               s("nothing to compare to")));
    assert_eq!(err("> 1"),                        (">",               s("expected a tag name")));
    assert_eq!(err("a.len > b"),                  ("b",               s("right side has to be a number")));
}

#[test]
fn parse_ok() {
    let ok = |q: &'static str| {
        let toks = tokenize(q).unwrap();
        Parser::parse(q, &toks).unwrap().to_string()
    };

    assert_eq!(ok("a == b"),                           "(a == b)");
    assert_eq!(ok("a is 'ed sheeran'"),                "(a is 'ed sheeran')");
    assert_eq!(ok("a == \"Guns N' Roses\""),           "(a == \"Guns N' Roses\")");
    assert_eq!(ok("a == 50"),                          "(a == 50)");
    assert_eq!(ok("a.len >= 2"),                       "(a.len >= 2)");
    assert_eq!(ok("a.len == 1"),                       "(a.len == 1)");
    assert_eq!(ok("a.len is not 2"),                   "(a.len is not 2)");
    assert_eq!(ok("!(a == b)"),                        "(! (a == b))");
    assert_eq!(ok("not not (a == b)"),                 "(not (not (a == b)))");
    assert_eq!(ok("not a == b"),                       "(not (a == b))");
    assert_eq!(ok("! a.len > 1"),                      "(! (a.len > 1))");
    assert_eq!(ok("not not a is 'ed sheeran'"),        "(not (not (a is 'ed sheeran')))");
    assert_eq!(ok("not a == b && c == d"),             "((not (a == b)) && (c == d))");
    assert_eq!(ok("a == b || c == d || e == f"),       "(((a == b) || (c == d)) || (e == f))");
    assert_eq!(ok("a == b and c == d or e == f"),      "(((a == b) and (c == d)) or (e == f))");
    assert_eq!(ok("a == b || c == d && e.len < 3"),    "((a == b) || ((c == d) && (e.len < 3)))");
    assert_eq!(ok("(a == b || c == d) && !(e == f)"),  "(((a == b) || (c == d)) && (! (e == f)))");
    assert_eq!(ok("genre.str contains 'hip hop'"),     "(genre.str contains 'hip hop')");
    assert_eq!(ok("genre.str is not rap"),             "(genre.str is not rap)");
    assert_eq!(ok("genre.str not contains rap"),       "(genre.str not contains rap)");
    assert_eq!(ok("not genre.str contains rap"),       "(not (genre.str contains rap))");
    assert_eq!(ok(""),                                 "false");
}

#[test]
fn eval_song() {
    use metaflac::block::VorbisComment;
    use std::path::PathBuf;

    let mut tags = VorbisComment::new();
    tags.set("ARTIST", vec!["Eminem", "Rihanna"]);
    tags.set("TITLE", vec!["Love The Way You Lie"]);
    let song = Song { path: PathBuf::new(), lyrics: None, tags: Some(tags) };
    let untagged = Song { path: PathBuf::new(), lyrics: None, tags: None };

    let matches = |q: &'static str, song: &Song| {
        let toks = tokenize(q).unwrap();
        song.matches(&Parser::parse(q, &toks).unwrap())
    };

    assert!( matches("artist == eminem", &song));
    assert!( matches("ARTIST is RIHANNA", &song));
    assert!(!matches("artist != eminem", &song));
    assert!(!matches("not artist == eminem", &song));
    assert!( matches("artist.len == 2", &song));
    assert!(!matches("artist.len > 2", &song));
    assert!( matches("title == 'love the way you lie'", &song));
    assert!( matches("genre.len == 0", &song));
    assert!(!matches("genre == rock", &song));
    assert!( matches("genre != rock", &song));
    assert!(!matches("genre.len > 0 && genre != rock", &song));
    assert!( matches("artist == drake || artist == eminem && title.len == 1", &song));

    let mut tags = VorbisComment::new();
    tags.set("GENRE", vec!["Hip Hop", "Pop Rap"]);
    let rap = Song { path: PathBuf::new(), lyrics: None, tags: Some(tags) };

    assert!( matches("genre.str contains 'hip hop'", &rap));
    assert!( matches("genre.str contains RAP", &rap));
    assert!(!matches("genre.str contains rock", &rap));
    assert!( matches("genre.str == 'pop rap'", &rap));
    assert!(!matches("genre.str == rap", &rap));
    assert!( matches("genre.str != rock", &rap));
    assert!(!matches("genre.str contains rap", &untagged));
    assert!(!matches("genre.str not contains rap", &rap));
    assert!( matches("genre.str not contains rock", &rap));
    assert!( matches("genre.str not contains rap", &untagged));

    assert!( matches("artist.len == 0", &untagged));
    assert!(!matches("artist == eminem", &untagged));
}

#[test]
fn parse_expr_err() {
    let err = |q: &'static str| {
        let toks = tokenize(q).unwrap();
        let e = Parser::parse(q, &toks).unwrap_err();
        (&q[e.ranges[0].start..e.ranges[0].end], e.reason.into_owned())
    };
    let quote = |s: &'static str| (s, format!("values with spaces have to be quoted, like `'{s}'`"));

    assert_eq!(err("artist == simon and garfunkel"),                quote("simon and garfunkel"));
    assert_eq!(err("artist == simon and garfunkel or a == b"),      quote("simon and garfunkel"));
    assert_eq!(err("artist == guns and roses && year == 1987"),     quote("guns and roses"));
    assert_eq!(err("artist == the guns and roses"),                 quote("the guns and roses"));
    assert_eq!(err("artist is not earth wind and fire"),            quote("earth wind and fire"));
    assert_eq!(err("genre.str contains hip and hop"),               quote("hip and hop"));
    assert_eq!(err("a == b and c.len"),                             ("c.len", "expected a comparison, like `tagname == value`".to_string()));
    assert_eq!(err("a.len > 1 and c"),                              ("c", "expected a comparison, like `tagname == value`".to_string()));
}

#[test]
fn parse_unop_err() {
    let err = |q: &'static str| {
        let toks = tokenize(q).unwrap();
        let e = Parser::parse(q, &toks).unwrap_err();
        (&q[e.ranges[0].start..e.ranges[0].end], e.reason.into_owned())
    };

    assert_eq!(err("not ! && a == b"),      ("!",                     "expected a condition after `!`".to_string()));
    assert_eq!(err("not a b"),              ("b",                     format!("expected one of {CMP_SET_OPS:?}")));
    assert_eq!(err("not a.len"),            ("a.len",                 "expected a comparison, like `tagname == value`".to_string()));
    assert_eq!(err("! && a == b"),          ("!",                     "expected a condition after `!`".to_string()));
    assert_eq!(err("!()"),                  ("()",                    "what do you want me to do with it?".to_string()));
}

#[test]
fn parse_group_err() {
    let err = |q: &'static str| {
        let toks = tokenize(q).unwrap();
        let e = Parser::parse(q, &toks).unwrap_err();
        (&q[e.ranges[0].start..e.ranges[0].end], e.reason.into_owned())
    };

    let logical = format!("expected one of {BOOL_BINOPS:?}");
    let empty = "what do you want me to do with it?".to_string();
    assert_eq!(err("(a == b) == c"),        ("== c", format!("{logical} followed by a condition")));
    assert_eq!(err("(a == b) (c == d)"),    (") (", logical.clone()));
    assert_eq!(err("(a == b) c == d"),      (") c", logical.clone()));
    assert_eq!(err("(a == b) !(c == d)"),   (") !", logical));
    assert_eq!(err("()"),                   ("()", empty.clone()));
    assert_eq!(err("(())"),                 ("()", empty.clone()));
    assert_eq!(err("() == c"),              ("()", empty));
}

#[test]
fn parse_parens_err() {
    let err = |q: &'static str| {
        let toks = tokenize(q).unwrap();
        let e = Parser::parse(q, &toks).unwrap_err();
        (&q[e.ranges[0].start..e.ranges[0].end], e.reason.into_owned())
    };

    let unclosed = |s: &'static str| (s, "unclosed paren".to_string());
    let unmatched = |s: &'static str| (s, "unmatched paren".to_string());

    assert_eq!(err("(a == b"),                  unclosed("(a == b"));
    assert_eq!(err("((a == b)"),                unclosed("((a == b)"));
    assert_eq!(err("(a == b) && (c == d"),      unclosed("(c == d"));
    assert_eq!(err("a == b)"),                  unmatched("a == b)"));
    assert_eq!(err(")("),                       unmatched(")"));
    assert_eq!(err("(a == b)) && (c == d"),     unmatched("(a == b))"));

    let q = "(a == b) && (c == d";
    let toks = tokenize(q).unwrap();
    assert_eq!(Parser::parse(q, &toks).unwrap_err().to_string(), "\n'(a == b) && (c == d'\n             ^^^^^^^\n             unclosed paren\n");
}
