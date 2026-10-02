---
name: mandate-mode
description: How agents work in the Mandate repository. Use at the start of any non-trivial task here (a backlog story, a spec change, a bug, a review, shipping a PR, or correcting a repeated agent mistake), and for /mandate-mode.
---

# Mandate mode

The goal is work the founder can trust without watching: small verified changes that agents
land themselves, decisions recorded rather than waited on, and every repeated correction turned
into structure. The founder is not a gate (DEC-79).

## Precedence

`AGENTS.md` and accepted decisions (`docs/project/04-decision-log.md`) come first, then this
skill, then the vendored skills (`how`, `why`, `tdd`, `blast-radius`, `interrogate`, `unslop`,
`technical-writing`, `deslop`, and for `web/` the design skills `impeccable`, `emil-design-eng`,
`review-animations`, `prototype`, `apple-design`, `baseline-ui`, `fixing-accessibility`,
`fixing-motion-performance`, and `web-design-guidelines`; see `.cursor/third_party/README.md`).

## Decide, record, continue

Do not wait for the founder. When a stop condition fires (a spec ambiguity or error, a new
dependency, a deviation from an accepted decision), write the decision as its own file,
`docs/project/decisions/DEC-<n>.md` (DEC-344; the directory's README has the format and how to
take a number), choose, and keep going:

- engineering and process decisions: set the status to Accepted, name yourself and the delegation
  under "Decided by", give the rationale and alternatives, then act on it;
- decisions reserved for the founder: set the status to Proposed, continue with the most
  conservative option, and list it at the top of your report.

The decision log (`docs/project/04-decision-log.md`) holds DEC-01 to DEC-302 and takes no new rows.

Reserved for the founder (DEC-79): anything involving live trading, live credentials, or real money
(never do these; AGENTS.md rule 8); spending or new paid services; legal and compliance text; and
weakening an approved safety invariant or an `AGENTS.md` non-negotiable. Never force-push `main`,
rewrite published history, or delete data.

Tests never get weaker to make a change pass. Protected paths still need a DEC and ship without
code (the spec guard enforces it).

## Start every task

1. Check the open claims (`gh issue list --label claim --state open`; `playbooks/coordination.md`)
   and claim your story. Then match the task to a playbook and open a todo list whose first items
   are its steps.
2. Read the documents the task touches (AGENTS.md "Sources of truth").
3. Name the data shapes before writing logic.

| Task | Playbook |
|---|---|
| Implement a backlog story | [`playbooks/story.md`](playbooks/story.md) |
| Change a spec, schema, reference case, or the reference implementation | [`playbooks/spec-change.md`](playbooks/spec-change.md) |
| The founder or a review corrects something an agent should never repeat | [`playbooks/correction.md`](playbooks/correction.md) |
| Ship any PR: independent review, then merge | [`playbooks/ship.md`](playbooks/ship.md) |
| Another coordinating session (Cursor cloud agents, Claude Code) is building at the same time | [`playbooks/coordination.md`](playbooks/coordination.md): claim first, split by crate, reserve IDs |
| How does X work, why is Y built this way | the `how` and `why` skills |
| A defect | the `tdd` skill: reproduce with a failing test, then fix the root cause |
| A small diff you do not fully trust | the `blast-radius` skill |

## Skills to reach for

- **Verification:** the `verify-mandate` skill. No claim without the command that proves it.
- **Design across a crate boundary:** write the caller's usage first, then the types and
  signatures as stubs; that stub API is the tests PR. For a contested shape, sketch two and compare.
- **Adversarial review of a safety-critical diff:** the `interrogate` skill, before opening the PR.
- **Prose (docs, PR descriptions, commits):** the `unslop` and `technical-writing` skills.
- **Before commit:** the `deslop` skill.
- **Web UI (`web/`, DEC-200):** no gradients, flat colour (founder, 2026-09-28). `impeccable` for
  direction, critique, audit, and polish; `emil-design-eng` and `review-animations` for motion;
  `prototype` to compare directions; `baseline-ui`, `fixing-accessibility`,
  `fixing-motion-performance`, and `web-design-guidelines` as review passes
  (`.cursor/third_party/README.md` says when to use which). The
  [product-experience brief](../../../docs/product/09-product-experience.md) wins where they differ.

## Subagents

Give each subagent file pointers and this skill, not pasted context. Review its diff yourself and
write your own summary; do not pass on what it said. Anything touching GitHub goes through a cloud
agent that holds the token, which it never prints or persists. Cloud agents cannot launch
subagents: a delegated story agent builds and opens PRs, and the coordinating session runs the
independent reviews (`playbooks/ship.md` step 3).

Push with a token-bearing URL only in single commands; never with `git push -u` or `--set-upstream`, which writes the token into `.git/config`. Cursor's cloud-agent hook that adds a `Co-authored-by` trailer can come back on new machines: run `chmod -x ~/.cursor/agent-hooks/*/commit-msg.cursor.co-author` before committing.

## Reporting

Lead with the outcome. Put evidence next to each claim (command and result). List the decisions
you made and the founder-reserved ones you deferred, and say what was deliberately left out.

At the end of a working session, update `docs/project/08-work-tracker.md`.
