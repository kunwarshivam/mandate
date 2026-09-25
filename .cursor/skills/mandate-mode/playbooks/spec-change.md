# Playbook: change a spec, schema, reference case, or the reference implementation

These paths are protected (ES-22) and founder-owned. A change needs a DEC and ships without code.

1. Separate decisions from defects. List design choices for the founder before drafting; fix only
   defects without asking. After an external review, fix only blockers and majors (freeze rule).
2. Write or update the invariants first, then the rules (AGENTS.md "Getting it right the first
   time").
3. Make every new claim a test in `reference/mandate/`, with an independent oracle shown to fail
   on a seeded bug.
4. Run `cargo xtask ci reference`, several fuzz seeds, and `reference/mandate/mutants.py`.
5. Trace every cross-spec reference and update both sides in the same change.
6. Regenerate fixtures with `cargo xtask refcases --write`; never edit `fixtures/refcases/` by hand.
7. Open the PR citing the DEC. Code that implements the change goes in a later PR.
