# Playbook: turn a correction into structure

Use when the founder or a review corrects something an agent did and it should never happen again.
A correction that stays in chat is lost; one written only as prose is followed sometimes. Put it on
the highest rung of the trust ladder that can hold it (AGENTS.md "The trust ladder").

1. State the mistake as one rule, in one sentence.
2. Walk the ladder from the top and stop at the first rung that can enforce the rule:
   1. **Make it unrepresentable:** a type, a private field, a module boundary, or a rule in
      `xtask/layers.toml`.
   2. **Check it statically:** a clippy lint in `Cargo.toml` or `clippy.toml`, or an xtask check
      wired into `cargo xtask ci lint`.
   3. **Guide it:** `AGENTS.md`, a skill under `.cursor/skills/`, or `.cursor/BUGBOT.md`.
   4. **Review it:** a line in `.github/pull_request_template.md`.
3. Implement the rung. Plant a violation, show the check fails, and remove it.
4. Fix existing violations, or, if that is large, keep the check failing only on new code and add a
   backlog item for the rest. Stopping the spread comes first.
5. Delete prose that the new structure now enforces, so the rule lives in one place.
6. In the PR, name the rung chosen and why the rungs above it could not hold the rule.
