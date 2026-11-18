# Locale and theme token conflict boundaries

This round audits the existing locale and theme-token rules. No IR field,
emitter protocol or token namespace was changed.

## Source basis

Flutter's engine `Locale` documents the Unicode language identifier shape: a
lowercase primary language subtag of two to eight letters that must not be
four letters long, plus case-sensitive region and script subtags
(`bin/cache/pkg/sky_engine/lib/ui/platform_dispatcher.dart:2878-2902`,
Flutter 3.50.0, commit `ab2912035e160d9cad6306df307576da6128ebdb`). Opurom
accepts the narrower BCP-47 form `^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$` and
reports the pattern in the help line of every rejected tag.

## Token sources and override order

Theme tokens come from two sources:

1. inline `theme { tokens { name: "#RRGGBB" } }` entries, in source order;
2. imported JSON files listed with `tokens { import "theme/x.json" }`.

Rules pinned by `tests/locale_theme_tokens.rs`:

- inline duplicates resolve last-wins, and the IR keeps one entry per name;
- imported tokens are flattened, sorted by name and merged into the same
  `theme.tokens` object;
- an imported token that collides with an inline token, or with a token from
  another import, is rejected with `duplicate token '<name>'` and no IR;
- imported tokens must use the `md-` namespace; other names are rejected;
- a 500-character token name and a 500-character catalog key lower
  deterministically to byte-identical IR.

## Locale rules

- invalid tags (`EN`, `abcd`, `e`, `en-abcdefghij`, `EN-us`) are rejected with
  the tag in the message and the accepted pattern in the help line;
- `en`, `zh-CN`, `zh-Hant-TW` and `en-Latn` are accepted and round-trip
  through `locale.active`;
- `supported` must not repeat a tag, `active` may be declared once, and the
  active tag must appear in `supported`;
- a catalog may declare a key once per locale.

## Alternatives considered

1. Accept four-letter primary subtags. Deferred: Flutter's engine `Locale`
   explicitly rejects a four-letter language subtag, so the current two-to-three
   letter rule stays.
2. Cap token or catalog key length. Deferred: no source in Flutter or Dart
   imposes such a limit, and adding one would change the accepted source
   language without a protocol review.
3. Let inline tokens override imported ones silently. Rejected: an accidental
   duplicate would silently change the palette, so the compiler reports it.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test locale_theme_tokens
```
