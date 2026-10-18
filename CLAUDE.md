# mql

Query language for filtering a music library by tags, e.g. `artist == eminem && artist.len > 1`.
The DSL lives in `src/dsl.rs` (`cargo test --lib`): `validate_quotes` + tokenizer, then `Parser` (stateless,
`eval`-style range splitting) into a type-safe `Loc<Expr>` tree, then `eval`. `Song`/tag lookup
(`Song::tag`, case-insensitive names) and `Songs::filter` are in `src/song.rs`. `cargo run` filters the
real library with the query in `src/bin/cli.rs`. `scratch/` holds older drafts, only for reference.

Run tests as `cargo test --lib --no-run`, then `timeout 1 cargo test --lib` (a parser loop once hung).

## Current goal: web UI demo (Friday)

Scope until the demo is the web UI and polish, nothing else.
- Server: `src/bin/server/` (`cargo run --bin server`; plain `cargo run` is the CLI via `default-run`).
  `server.rs` is generic hand-written HTTP (read request, `respond`, URL decoding, LAN IP), no MQL in it.
  `main.rs` is the app: startup, explicit routes only (no path-to-file mapping), `run_query`.
  `render.rs` builds the data-driven HTML (song table, errors) with `maud`, which escapes everything in `( )`.
  The static page lives in `web-ui/` (`index.html`, `style.css`, `main.js`), read on every request.
  Class names are the contract between `render.rs` and `style.css`. Library is read once at startup.
- Frontend: plain HTML, a little CSS (full-width textarea, `<hr>`, output below, red on error, table
  styling, monospace at least for errors), and ~10 lines of JS.
- Live updates: on input, debounce 100 ms, `fetch` the query, server returns either the rendered error
  or the song list as an HTML fragment. A sequence number drops out-of-order responses. Chosen over
  WebSockets: a hand-written server would need the handshake and framing; parsing and filtering in memory
  take well under a millisecond.
- Errors go in a `<pre>`: caret alignment needs monospace. If errors are ever highlighted in the textarea
  itself, convert byte ranges to JS UTF-16 offsets (Cyrillic and `ó` differ).
- "Open in Deezer": songs have `SOURCE=Deezer`, `SOURCEID=<id>`, link is `https://www.deezer.com/track/<id>`.

## Language spec (decided)

- Grammar is a fixed shape parsed by splitting on the lowest-precedence logical op outside parens
  (`||` < `&&` < `!`, left-associative). Chosen over Pratt deliberately: readability over cleverness.
- A query must be a condition. A bare tag is never a boolean: `a && (b == c)` is an error.
- Parens may only wrap conditions. `(artist).len`, `artist == (eminem)`, `(a == b) == c` are errors.
- Comparisons don't chain or nest; only `and/&&`, `or/||`, `not/!` combine conditions.
- `is` = `==`, `is not` = `!=`.
- `not`/`!` negates the condition right after it, parens optional: `not a == b && c == d` is `(not (a == b)) && (c == d)`.
  A bare tag is never a boolean, so `(not a) == b` isn't a possible reading. (Requiring parens was tried and dropped.)
- The lhs decides the kind of comparison: a tag is a list, `tag.len` is a number.
- `==`/`!=`: lhs is a tag name and rhs is a value (a bare word on the rhs is always a string),
  or lhs is `tag.len` and rhs is a number (`artist.len == 1`).
  Values with spaces must be quoted (`'ed sheeran'`); the error for unquoted multi-word values suggests quotes.
- `>`, `<`, `>=`, `<=`: lhs is `tag.len` (the only property), rhs is a number.
  Tag always goes on the left: `1 < a.len`, `eminem == artist`, `a.len > b.len` are errors.
- `in` was removed. Future idea: `(a, b, c) in artist` meaning any-of, maybe `all`, `not in`.
  Don't implement until asked.
- A run of word chars that starts with an ASCII digit and has only digits and `_` is a number (`1_000`),
  anything else is a word (`2pac`, `50cent`, `_1`).
  Words and tag names are Unicode letters/digits and `_` (`album_artist`).
- Quotes are checked before tokenizing (`validate_quotes`). An odd number of one quote kind is an error that
  marks every quote of that kind and doesn't guess the fix (`Guns N' Roses`, `'it's'`, `'Björk`).
  With even counts, a quote glued to a letter or digit (`'Rock 'n' Roll'`, `'x'and`) is an error at that quote,
  suggesting the other quote type. A quote of the other kind inside a value is a plain char (`"Guns N' Roses"`).
- Tag matching is case-insensitive on both name and value.
- Field access: one level, `tag.field`. `tag.len` is a number. `tag.str` is the tag's values as strings,
  per value like `==` (any value matches). `.str` takes `contains`, `not contains`, `does not contain`,
  and `==`/`!=`/`is`/`is not`. `contains` is a substring match, so `contains rap` also matches `trap`.
- Ops come from lists (`CMP_SET_OPS`, `CMP_NUM_OPS`, `CMP_STR_OPS`, ...), not enums: iterable for
  "expected one of [...]" errors and matched by string in eval. Multi-word ops (`is not`, `does not contain`)
  match whole phrases with any whitespace between words, so their words stay usable as values on their own.
  Single-word ops (`contains`, `and`, `or`) are reserved.
- Errors: "expected X" for a missing token, "did you mean / you can also write" for real ambiguity, and
  name the op and both types for mismatches (`` `contains` needs a string, `genre.len` is a number ``).
  Multiline queries get a line-number gutter; single-line ones keep the `'query'` form.
- Eval is infallible: a missing tag (or a song with no tags) is an empty list. `.len` is 0, `==` is false.
  `!=` is exactly `not ==` ("no value equals"), so it's true for an empty list. `artist.len == 0` finds
  songs without an artist; `genre.len > 0 && genre != rock` excludes untagged ones.

## Rejected ideas (don't re-propose)

- Unquoted multi-word values split on logical ops (`artist == ed sheeran && ...`): ambiguous with
  `artist == me or <expr>`, invites injection-like surprises.
- Tag-to-tag comparison via a sigil (`artist == $albumartist`): too cryptic, "not bash for music".
  Tag-to-tag comparison is out of scope for now; pre-scanning tags to disambiguate is also rejected.
- `!=` as "some value differs": `[eminem, rihanna]` would match `artist != eminem`.
- `doesn't contain`: the `'` is a glued quote and `validate_quotes` rejects it. Use `does not contain`.
- Trimming blank lines or common indentation in multiline errors: not worth the complexity.
- Tabs, right-to-left scripts and wide (CJK) chars in error alignment: out of scope. Char counting is right
  for Cyrillic and Polish, which is what matters.
- A missing tag in `tag.len` as a runtime error: makes `artist.len == 0` impossible.

## Open / proposed, not yet decided

- Typo detection as a library-wide warning ("no song has tag `artsit`"), checked once before filtering,
  possibly with a did-you-mean suggestion. Tag ranges are already in `Expr::Set`/`Expr::Num` (`Loc`).
  This is the "bulk runtime" level from the section below and the real job of `Songs::validate`.
- Per-song `is_valid`: with infallible eval it has nothing to check yet. Dropped for now; revisit when the
  bulk check gives it a job. `Songs::validate` can return no broken songs until then.
- An owned error type so `?` works into `anyhow` with non-'static queries.
- Did-you-mean for ops (`does no contain` → `does not contain`), same edit-distance machinery as tag typos.
- Word-based string ops (`has word rap`) if substring `contains` keeps matching `trap`.
- Artificial tags resolved in eval (see the Todo above `eval`): `lyrics`, later `year` / `release_date.year`.

## Future ideas (after the demo, don't start without being asked)

- Fuzzy search mode: switch the textarea into a plain search box (double-click was the first idea, but
  double-tap selects text on phones, so maybe a toggle or long-press). The font morphs from robotic to
  casual: the Recursive variable font has `MONO` and `CASL` axes, animatable with a CSS `transition` on
  `font-variation-settings`.
- The query language in the user's music player written in C.
