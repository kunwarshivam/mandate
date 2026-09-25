---
name: mandate-mode
description: How agents work in the Mandate repository. Use at the start of any non-trivial task here (a backlog story, a spec change, a bug, a review, landing PRs, or correcting a repeated agent mistake), and for /mandate-mode.
---

# Mandate mode

The goal is work the founder can trust without watching: small verified changes, decisions
surfaced instead of made silently, and every repeated correction turned into structure.

## Precedence

`AGENTS.md` and accepted decisions (`docs/project/04-decision-log.md`) come first, then this
skill, then any plugin skill (pstack, cursor-team-kit). Where a plugin skill says to proceed
without asking, to merge, or to skip planning, the gates below still hold.

## Always stop for the founder

Proceed on reversible work without asking. Stop, and write the question or a DEC proposal, for:

- merging a PR, force-pushing, or deleting a branch someone else may use;
- changing a protected path (`docs/specs/`, `schemas/`, `reference/`, `fixtures/refcases/`,
  `crates/mandate-refcases/status.toml`) without a DEC that authorizes it;
- a spec ambiguity or apparent spec error, a new dependency, a test that would have to weaken, or
  any deviation from an accepted decision (AGENTS.md "How to work", ADR-0001 ES-15 stop conditions);
- anything touching live credentials, real orders, or production data (never; AGENTS.md rule 8).

Founder-owned files (CODEOWNERS) may change in a PR; name each one in the PR description.

## Start every task

1. Match the task to a playbook and open a todo list whose first items are its steps.
2. Read the documents the task touches (AGENTS.md "Sources of truth").
3. Name the data shapes before writing logic.

| Task | Playbook |
|---|---|
| Implement a backlog story | [`playbooks/story.md`](playbooks/story.md) |
| Change a spec, schema, reference case, or the reference implementation | [`playbooks/spec-change.md`](playbooks/spec-change.md) |
| The founder or a review corrects something an agent should never repeat | [`playbooks/correction.md`](playbooks/correction.md) |
| Land a stack of PRs, or rebase one after a squash merge | [`playbooks/stack.md`](playbooks/stack.md) |
| How does X work, why is Y built this way | pstack `how` and `why` |
| A defect | pstack `tdd`: reproduce with a failing test, then fix the root cause |
| A small diff you do not fully trust | pstack `blast-radius` |

## Skills to reach for

- **Verification:** the `verify-mandate` skill. No claim without the command that proves it.
- **Design across a crate boundary:** pstack `architect`. The sketched API becomes the stub API of
  the tests PR.
- **Adversarial review of a safety-critical diff:** pstack `interrogate`, before opening the PR.
- **Prose (docs, PR descriptions, commits):** pstack `unslop` and `technical-writing`.
- **Before commit:** cursor-team-kit `deslop`.

## Subagents

Give each subagent file pointers and this skill, not pasted context. Review its diff yourself and
write your own summary; do not pass on what it said. Anything touching GitHub goes through a cloud
agent that holds the token, which it never prints or persists.

## Reporting

Lead with the outcome. Put evidence next to each claim (command and result). List decisions the
founder must make separately from what was done, and say what was deliberately left out.
