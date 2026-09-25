---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["landonxjames"]
references: []
breaking: true
new_feature: false
bug_fix: false
---

**Behavior change:** `BigInteger::from_str` and `BigDecimal::from_str` now
perform structural validation instead of only checking that every character is
permitted. Inputs without a single numeric reading, such as `"--5"`,
`"1.2.3"`, `"1e"`, and `"1e+"`, now return
`BigNumberError::InvalidFormat`. `BigInteger` also rejects a leading `+`.

The accepted grammars are:

```text
BigInteger := '-'? DIGIT+
BigDecimal := ('+' | '-')? ( DIGIT+ ( '.' DIGIT* )? | '.' DIGIT+ )
              ( ('e' | 'E') ('+' | '-')? DIGIT+ )?
```

`BigDecimal` therefore continues to accept forms such as `"+123"`, `".5"`,
and `"1."`. An exponent or resulting scale outside the supported range returns
`BigNumberError::ExponentOutOfRange`. Leading-zero forms remain accepted. When
arbitrary-precision values are emitted as raw JSON numbers, the JSON codec
separately rejects forms invalid under RFC 8259 and suggests
`JsonCodecSettings::use_string_for_arbitrary_precision`.
