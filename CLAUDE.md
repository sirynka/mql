# mql

Query language for filtering a music library by tags, e.g. `artist == eminem && artist.len > 1`.
The DSL is being moved into `src/dsl.rs` (`cargo test --lib`): the tokenizer is there, the parser and eval
still live in `scratch/_dsl_claude.rs`. `Song`/tag reading is in `src/song.rs`.

## Language spec (decided)

- Grammar is recursive descent over a fixed shape (see the comment above `Parser` in `scratch/_dsl_claude.rs`).
  Chosen over Pratt deliberately: readability over cleverness, the grammar is too restricted to need it.
- A query must be a condition. A bare tag is never a boolean: `a && (b == c)` is an error.
- Parens may only wrap conditions. `(artist).len`, `artist == (eminem)`, `(a == b) == c` are errors.
- Comparisons don't chain or nest; only `and/&&`, `or/||`, `not/!` combine conditions.
- `is` = `==`, `is not` = `!=`.
- `not a == b` is rejected as ambiguous; the error suggests `a != b` or `not (a == b)`. Negation needs parens.
- `==`/`!=`: lhs is a tag name, rhs is a value. A bare word on the rhs is always a string.
  Values with spaces must be quoted (`'ed sheeran'`); the error for unquoted multi-word values suggests quotes.
- `>`, `<`, `>=`, `<=`: lhs is `tag.len` (the only property), rhs is a number.
  Tag always goes on the left: `1 < a.len`, `eminem == artist`, `a.len > b.len` are errors.
- `in` was removed. Future idea: `(a, b, c) in artist` meaning any-of, maybe `all`, `not in`.
  Don't implement until asked.
- A run of letters/digits that is all ASCII digits is a number, anything else is a word (`2pac`, `50cent`).
  Words and tag names are Unicode letters/digits.
- A quote glued to a letter or digit (`5'nizza`, `'it's'`, `'x'and`) is an error at that quote, suggesting
  the other quote type; the error doesn't guess where the value ends.
- Tag matching is case-insensitive on both name and value.

## Rejected ideas (don't re-propose)

- Unquoted multi-word values split on logical ops (`artist == ed sheeran && ...`): ambiguous with
  `artist == me or <expr>`, invites injection-like surprises.
- Tag-to-tag comparison via a sigil (`artist == $albumartist`): too cryptic, "not bash for music".
  Tag-to-tag comparison is out of scope for now; pre-scanning tags to disambiguate is also rejected.

## Open / proposed, not yet decided

- Make `eval` infallible: missing tag = no values (`.len` 0, `==` false, `!=` true). Currently
  `.len` on a missing tag is the only runtime error, inconsistent with `==` returning false.
- Typo detection as a library-wide warning ("no song has tag `artsit`"), checked once before filtering,
  possibly with a did-you-mean suggestion. Needs tag ranges in `Expr::Eq`.
- User-facing API the user sketched: `parse_filter(q)?` then per-song `s.is_valid(q) && s.matches(q)`.
  Suggested instead: `Filter::parse(q)?` + `q.matches(&song)` with std `.filter()`, DSL moved into `src/`,
  and an owned error type so `?` works into `anyhow` with non-'static queries. User hasn't decided yet.


## Update to user facing api (from Sirynka)
It makes sense to have multiple levels of validation
- Parse time
- bulk runtime (to measure editing distance and suggest fixes for typo)
- per song runtime

the last thing is an actual filter

```rust
let songs = ...;
let q = Filter::parse("whatever");
let (songs, broken) = songs.validate(q);
let matched = songs.filter(q);
```

This would allow to report broken as whatever i'd decide (warning, error, suggestion to edit query)

```rust
for s in songs {
  if s.is_valid(q) && s.matches(q) {
    // push somewhere
  }
}
```

Would still be nice to have, as it would make `songs.validate(q)` and `songs.filter(q)` impl quite concise
and expose lower level primitives for people wanting to do more than the basic flow.

