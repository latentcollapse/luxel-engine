# WGE language conformance data

This directory contains hand-authored, host-neutral contract vectors for the
model-native WGE language. They are normative test inputs referenced by
`docs/DSL docs/WGE_LANGUAGE_SPEC.md`; a production compiler may add
backend-specific fixtures but must not regenerate these expected values from
the implementation under test.

- `closure_v06.json` closes the draft 0.6 integrity ledger: ordering, digest
  membership, module/profile/compiler identity, syntax, numeric conversion,
  repair isolation, and bounded canonical details.
- `migration_worldbuilder_v1.json` dispositions every test in the current
  `worldbuilder_dsl.py` prototype as preserve, replace, or retire.
- `tests/test_wge_language_contract.py` validates the vectors with the Python
  standard library without choosing a production compiler host.
- `freeze_v06.json` pins the exact frozen core-contract evidence set, scoped
  exclusions, independent verdict, and verification command. It deliberately
  does not claim WGE language 1.0 release conformance.

Canonical IR remains JCS JSON. These files are readable fixture sources, not a
second serialization authority.
