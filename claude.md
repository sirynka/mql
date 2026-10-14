- Rewrite the parser to stateless style: functions take the full `toks` slice
  plus a `rest: Range<usize>` and return `(Expr, parsed: Range<usize>)`.
  No Parser struct, no mutable index.
- Grammar, error messages, and tests stay as they are; tests must pass unchanged.
- AST nodes should eventually carry token ranges so runtime errors use
  `Error::from_toks` too (see the `Num::Len` thread). Type checks stay at parse time.


Note to Sirynka, not Claude: We can finally start working on moving the parser
