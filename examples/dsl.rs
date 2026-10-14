use std::io::{Write, stderr};
#[allow(unused)]
use std::{borrow::Cow, fmt::Display, ops::Range, process::exit};
use thiserror::Error;
use itertools::Itertools;
use mql::{Song, read_songs_in_dir, anyhow_songs_to_ok_err};

#[derive(Debug, PartialEq)]
enum TokenType<'q> {
    BinOp(&'q str),
    UnOp(&'q str),
    QuotedLit(&'q str),
    Lit(&'q str),
    Number(u32),
    OParen,
    CParen,
}

#[derive(Debug, PartialEq)]
struct Token<'q> {
    ty: TokenType<'q>,
    str: &'q str,
}

#[derive(Debug, Error)]
struct Error<'q> {
    query: &'q str,
    range: Range<usize>,
    reason: Cow<'q, str>,
}

impl<'q> Error<'q> {
    pub fn from_toks<'t>(query: &'q str, toks: &'t [Token<'q>], reason: impl Into<Cow<'q, str>>) -> Error<'q> {
        let first = &toks[0];
        let last = &toks[toks.len() - 1];
        let idx = first.str.as_ptr() as usize - query.as_ptr() as usize;
        let len = last.str.as_ptr() as usize - first.str.as_ptr() as usize + last.str.len();
        Error {
            query,
            range: idx..idx + len,
            reason: reason.into(),
        }
    }
}

impl Display for Error<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let spaces = " ".repeat(self.range.start);
        let arrows = "^".repeat(self.range.len());
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
    macro_rules! dot    { () => { Token { ty: TokenType::Dot, .. } }; }
    macro_rules! oparen { () => { Token { ty: TokenType::OParen, .. } }; }
    macro_rules! cparen { () => { Token { ty: TokenType::CParen, .. } }; }

    pub(crate) use { binop, unop, qlit, lit, number, dot, oparen, cparen };
}

mod node {
    macro_rules! lit    { ($v: pat) => { Node { ty: NodeType::Lit($v), .. } }; }
    macro_rules! binop  { () => { Node { ty: NodeType::BinOp { .. }, .. } }; }
    macro_rules! number {
        () => { Node { ty: NodeType::Number(_), .. } };
        ($v: pat) => { Node { ty: NodeType::Number($v), .. } };
    }
    // macro_rules! unop   { ($v: pat) => { Node { ty: NodeType::UnOp($v), ..} }; }
    // macro_rules! booll    { ($v: pat) => { Node { ty: NodeType::Bool($v), ..} }; }

    pub(crate) use { lit, binop, number};
}

const BIN_OPS: &[&str] = &["||", "or", "&&", "and", "in", "!=", "==", "is", ">=", "<=", ">", "<", "."];
const UN_OPS: &[&str] = &["not", "!"];

fn tokenize<'q>(q: &'q str) -> Result<Vec<Token<'q>>, Error<'q>> {
    macro_rules! consume_if_starts_with {
        ($p:expr, $ty:expr, $q:expr, $c:expr) => {
            if $q[$c..].starts_with($p) {
                return Ok((Token { ty: $ty, str: &$q[$c..$c+$p.len()] }, $c+$p.len()));
            }
        };
    }

    fn consume<'q>(q: &'q str, mut c: usize) -> Result<(Token<'q>, usize), Error<'q>> {
        while c < q.len() && q[c..].starts_with(|c: char| c.is_whitespace()) { c += 1; }

        consume_if_starts_with!("(", TokenType::OParen, q, c);
        consume_if_starts_with!(")", TokenType::CParen, q, c);

        for op in UN_OPS  { consume_if_starts_with!(op, TokenType::UnOp(op), q, c); }
        for op in BIN_OPS { consume_if_starts_with!(op, TokenType::BinOp(op), q, c); }

        if q[c..].starts_with('\'') {
            let e = q[c+1..].find('\'').ok_or(Error { query: q, reason: Cow::Borrowed("unterminated quote"), range: c..q.len() })?;
            return Ok((Token { ty: TokenType::QuotedLit(&q[c+1..c+1+e]), str: &q[c..c+2+e] }, c+2+e));
        }

        if q[c..].starts_with('"') {
            let e = q[c+1..].find('"').ok_or(Error { query: q, reason: Cow::Borrowed("unterminated quote"), range: c..q.len() })?;
            return Ok((Token { ty: TokenType::QuotedLit(&q[c+1..c+1+e]), str: &q[c..c+2+e] }, c+2+e));
        }

        if q[c..].starts_with(|c: char| c.is_ascii_digit()) {
            let e = q[c..].find(|c: char| !c.is_ascii_digit()).unwrap_or(q.len()-c);
            let n = q[c..c+e].parse().map_err(|err| Error { query: q, reason: Cow::Owned(format!("{err}")), range: c..c+e })?;
            return Ok((Token { ty: TokenType::Number(n), str: &q[c..c+e] }, c+e));
        }

        if q[c..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            let e = q[c..].find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(q.len()-c);
            return Ok((Token { ty: TokenType::Lit(&q[c..c+e]), str: &q[c..c+e] }, c+e));
        }

        Err(Error { query: q, reason: Cow::Borrowed("unexpected token"), range: c..c+1 })
    }

    let mut c = 0;
    let mut t: Token<'_>;
    let mut toks = Vec::new();

    while !q[c..].is_empty() {
        (t, c) = consume(q, c)?;
        toks.push(t);
    }

    Ok(toks)
}

#[derive(Debug)]
enum NodeType<'q> {
    BinOp {
        op: &'q str,
        lhs_and_rhs: Box<(Node<'q>, Node<'q>)>,
    },
    UnOp {
        op: &'q str,
        expr: Box<Node<'q>>,
    },
    Number(u32),
    Lit(&'q str),
    Bool(bool),
}

#[derive(Debug)]
struct Node<'q> {
    ty: NodeType<'q>,
    parsed: Range<usize>,
    rest: Range<usize>,
}

impl<'q> Node<'q> {
    fn b_false() -> Node<'q> {
        Node {
            ty: NodeType::Bool(false),
            parsed: 0..0,
            rest: 0..0,
        }
    }

    fn u32(&self, q: &'q str, toks: &[Token<'q>], song: &Song) -> Result<u32, Error<'q>> {
        use node::*;
        Ok(match &self.ty {
            NodeType::Number(n) => *n,
            NodeType::BinOp { op, lhs_and_rhs } => {
                let (lhs, rhs) = lhs_and_rhs.as_ref();
                match (op, &song.tags, lhs, rhs) {
                    (&".", Some(tags), lit!(key), lit!("len")) =>
                        match tags.get(&key.to_uppercase()) {
                            Some(tags) => tags.len() as u32,
                            None => Err(Error::from_toks(q, &toks[self.parsed.start..self.parsed.end], format!("tag does not exist in {:?}", song.name().unwrap())))?,
                        }

                    (op, _, lhs, rhs) => unimplemented!("{lhs} \x1b[41m{op}\x1b[0m {rhs}"),
                }
            },
            t => unimplemented!("{t:#?}"),
        })
    }

    fn bool(&self, q: &'q str, toks: &[Token<'q>], song: &Song) -> Result<bool, Error<'q>> {
        use node::*;

        Ok(match &self.ty {
            NodeType::Bool(b) => *b,
            NodeType::BinOp { op, lhs_and_rhs } => {
                let (lhs, rhs) = lhs_and_rhs.as_ref();
                match (op, &song.tags, lhs, rhs) {

                    (&"==", Some(tags), lit!(key), lit!(val)) =>
                        match tags.get(&key.to_uppercase()) {
                            Some(tags) => tags.iter().any(|tag| tag.to_lowercase() == val.to_lowercase()),
                            None => false,
                        },

                    (&"&&", _, binop!(), binop!()) => lhs.bool(q, toks, song)? && rhs.bool(q, toks, song)?,
                    (&"||", _, binop!(), binop!()) => lhs.bool(q, toks, song)? || rhs.bool(q, toks, song)?,
                    (&">", _, binop!(), number!()) => lhs.u32(q, toks, song)? > rhs.u32(q, toks, song)?,

                    (op, _, lhs, rhs) => unimplemented!("{lhs} \x1b[41m{op}\x1b[0m {rhs}"),
                }
            },
            t => unimplemented!("{t:#?}"),
        })
    }
}

fn validate_parens<'t, 'q: 't>(query: &'q str, toks: &'t [Token<'q>]) -> Result<(), Error<'q>> {
    let mut n = 0;
    let mut last_balanced_idx = 0;

    for (i, t) in toks.iter().enumerate() {
        match t.ty {
            TokenType::OParen => n += 1,
            TokenType::CParen => n -= 1,
            _ => {}
        }

        match n {
            ..0 => return Err(Error::from_toks(query, &toks[..i+1], "Too many closing parens")),
            0 => last_balanced_idx = i + 1,
            _ => {}
        }
    }

    match n {
        0 => Ok(()),
        1.. => Err(Error::from_toks(query, &toks[last_balanced_idx..], "Unclosed paren")),
        _ => unreachable!()
    }
}

#[allow(unused)]
fn skip_paren_group(toks: &[Token], range: Range<usize>) -> usize {
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

fn find_op_idx_with_min_prec(toks: &[Token], range: Range<usize>) -> Option<usize> {
    if range.is_empty() { return None; }

    let mut min_idx = 0;
    let mut min_prec = usize::MAX;

    for (i, t) in toks[range.start..range.end].iter().enumerate() {
        let prec = BIN_OPS.iter().position(|op| *op == t.str).unwrap_or(usize::MAX);

        if prec < min_prec {
            min_prec = prec;
            min_idx = i;
        }
    }

    match min_prec == usize::MAX {
        false => Some(min_idx + range.start),
        true => None,
    }
}

fn parse_binop<'t, 'q: 't>(q: &'q str, toks: &'t[Token<'q>], range: Range<usize>, idx: usize) -> Result<Node<'q>, Error<'q>> {
    let lhs = parse_impl(q, &toks, range.start..idx)?;
    let rhs = parse_impl(q, &toks, idx+1..range.end)?;

    Ok(Node {
        ty: NodeType::BinOp {
            op: toks[idx].str,
            lhs_and_rhs: Box::new((lhs, rhs))
        },
        parsed: range,
        rest: 0..0,
    })
}

fn parse_impl<'t, 'q: 't>(q: &'q str, toks: &'t[Token<'q>], range: Range<usize>) -> Result<Node<'q>, Error<'q>> {
    use tok::*;
    match find_op_idx_with_min_prec(toks, range.start..range.end) {
        Some(i) => parse_binop(q, toks, range.start..range.end, i),
        None => Ok(match &toks[range.start..range.end] {
            [lit!(lit)] => Node {
                ty: NodeType::Lit(lit),
                parsed: range.start..range.end,
                rest: range.end..range.end,
            },

            [number!(a)] => Node {
                ty: NodeType::Number(*a),
                parsed: range.start..range.end,
                rest: range.end..range.end,
            },

            tt => todo!("{tt:#?}")
        }),
    }
}

fn parse<'t, 'q: 't>(q: &'q str, toks: &'t [Token<'q>]) -> Result<Node<'q>, Error<'q>> {
    if toks.is_empty() { return Ok(Node::b_false()); }
    validate_parens(q, toks)?;
    parse_impl(q, toks, 0..toks.len())
}

// Error<'q> is not quite true for eval error
// It should be shorter of ('s, 'q)
fn try_filter<'s, 'q, 't>(songs: &'s [Song], query: &'q str, toks: &'t [Token<'q>], ast: Node<'q>) -> Vec<Result<&'s Song, Error<'q>>> {
    songs
        .iter()
        .filter_map(|s| match ast.bool(query, toks, s) {
            Ok(true) => Some(Ok(s)),
            Err(e) => Some(Err(e)),
            Ok(false) => None,
        })
        .collect_vec()
}

fn filtered_songs_to_ok_err<'s, 'q>(songs: Vec<Result<&'s Song, Error<'q>>>) -> (Vec<&'s Song>, Vec<Error<'q>>) {
    let (ok, err): (Vec<_>, Vec<_>) = songs.into_iter().partition(Result::is_ok);
    let ok = ok.into_iter().map(Result::unwrap).collect_vec();
    let err = err.into_iter().map(Result::unwrap_err).collect_vec();
    (ok, err)
}

impl std::fmt::Display for Node<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.ty {
            NodeType::Number(n) => write!(f, "{n}"),
            NodeType::BinOp { op, ref lhs_and_rhs } => write!(f, "({} {op} {})", lhs_and_rhs.0, lhs_and_rhs.1),
            NodeType::UnOp { op, ref expr } => write!(f, "({op} {})", expr),
            NodeType::Lit(lit) => write!(f, "{lit}"),
            NodeType::Bool(b) => write!(f, "{b}"),
        }
    }
}

fn main() -> anyhow::Result<()> {
    let songs = read_songs_in_dir("/home/sirynka/Music/Liked/")?;
    let (songs, _) = anyhow_songs_to_ok_err(songs);

    let q = "artist == eminem && artist.len > 1";
    let toks = tokenize(q)?;
    let ast = parse(q, &toks)?;
    let songs = try_filter(&songs, q, &toks, ast);
    let (songs, errors) = filtered_songs_to_ok_err(songs);

    if let Some(e) = errors.first() {
        let spaces = " ".repeat(e.range.start);
        let arrows = "^".repeat(e.range.len());
        writeln!(stderr(), "'{}'", e.query)?;
        writeln!(stderr(), " {spaces}{arrows}")?;
        writeln!(stderr(), " {spaces}{}", e.reason)?;
        writeln!(stderr(), " {spaces}and {} other songs", errors.len() - 1)?;
    }

    for s in songs { println!("{:?}", s.name().unwrap()); }
    Ok(())
}

// "Akon - Smack That"
// "Dr. Dre - I Need A Doctor"
// "Ed Sheeran - Remember The Name (feat. Eminem & 50 Cent)"
// "Eminem - Beautiful Pain"
// "Eminem - Black Magic"
// "Eminem - Love The Way You Lie"
// "Eminem - Nowhere Fast"
// "Eminem - Stan"
// "Eminem - Superman"
// "Eminem - The Monster"
// "Eminem - Till I Collapse"
// "Skylar Grey - C’mon Let Me Ride"
// "Skylar Grey - Last One Standing (From Venom_ Let There Be Carnage)"
