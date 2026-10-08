# Playbook: work alongside other coordinating sessions

Several coordinating sessions build this repository at once: Claude Code sessions run by the
founder, Cursor cloud agents, and the builders and reviewers each of them launches. GitHub is the
only state they share, so every claim, reservation, and handoff lives there, never in a chat.

## 1. Claim before you start

- One GitHub issue per story or task, labeled `claim`, titled `<story ID>: <title>`. The body
  names the coordinator (`claude-code` or `cursor`), the branch names, and the stage (brief, tests
  PR, implementation PR, status PR, spec change, spike).
- Before picking anything up, run `gh issue list --label claim --state open`. A story with an
  open claim is taken; do not touch its crates, tests, or documents.
- Two claims for one story: the lower issue number wins; the other closes with a comment.
- Close the claim when the story's last PR merges. Update its stage as PRs open and merge.

## 2. Split by crate

The current allocation. Change it by editing this table in a PR, never by starting work.

| Coordinator | Stories | Crates and paths |
|---|---|---|
| `claude-code` | Everything from 2026-09-26 evening except `web/` (DEC-200): E4-1, E4-2, E5-4, E2-3, E17-0 (spike), the DEC-97 and DEC-98 spec rewrite (Track C), the M1 exit run, and every follow-up; the `cursor` row below is paused at the founder's request and holds nothing new | `mandate-accounting`, `mandate-sim`, `python/research_spike/`, `docs/specs/mandate.md`, `schemas/mandate.schema.json`, `schemas/policy.schema.json`, `reference/mandate/` |
| `cursor` | W3, the web UI (DEC-200; claims [#246](https://github.com/kunwarshivam/mandate/issues/246) and [#247](https://github.com/kunwarshivam/mandate/issues/247)), from 2026-09-28. Its earlier backend stories passed to `claude-code` with the pause of 2026-09-26 | `web/` (the web UI, DEC-200), its workflow `.github/workflows/web.yml`, and vendored design skills under `.cursor/skills/`; merges UI PRs itself (founder, 2026-09-28) |

Shared, owned by no one: `mandate-num`, `mandate-time`, `mandate-canon`, `xtask`, and
`mandate-refcases` with `status.toml` and `docs/specs/reference-cases/`. In the harness and the
case files, ownership is per case: each story edits only the interpretations, cases, and status
rows it names in its claim. A change to a shared crate needs its own claim issue, stays minimal,
and says which story needs it.

## 3. Reserve identifiers before you use them

Decision IDs (`DEC-`), open decisions (`OD-`), ADR numbers, and epic and story IDs collide
silently between sessions: two open PRs have already minted the same DEC numbers.

**Decision IDs (DEC-344).** Each decision is a file, `docs/project/decisions/DEC-<n>.md`, and its
[README](../../../../docs/project/decisions/README.md) is the procedure: take the range your
dispatch gives you, or the next integer after the highest on `main`; add the file with the status
`Reserved` in your first commit; fill it in when you decide. The decision log takes no new decision
or `DEC-` reservation rows, and a PR edits only its own decision files. Two PRs that take the same
number add the same path and cannot both merge, so the second renumbers.

**Every other identifier.** The registry is the **Reserved identifiers** table at the end of
`docs/project/04-decision-log.md`. Before the first commit that uses a new identifier:

1. Take the next integer for that prefix after the highest one in the table or in the log's
   rows on `main`, whichever is larger. The table is on `main`, so fetch first. A block reserved
   ahead of the sequence (DEC-200 to DEC-249, `cursor`, the web UI) does not count as the highest;
   numbers below it run on, then jump past it.
2. Add a row (identifier, coordinator, claim issue, purpose) in a one-line docs PR, or in your
   claim's first PR if it is docs-only and can merge within the hour. Until that row is on
   `main`, also comment `DEC-<n> reserved` on your claim issue so the other side sees it.
3. When the decision merges, the row stays as the record; a reservation that is abandoned is
   marked "released" rather than deleted, and its number is never reused.

## 4. Shared files

The work tracker, the backlog, and `status.toml` are edited by everyone. (The decision log was one
of them until DEC-344 gave each decision its own file, and the feature map until it became one file
a feature under `.cursor/skills/verify-mandate/features/`: add a feature's file, or edit only your
own feature's.) In each: add or
change only the rows for your own stories; never rewrite another coordinator's rows; append
rather than reorder. Rebase onto `main` immediately before merging
and resolve conflicts by keeping both sides.

## 5. One merge queue

Cursor cloud agents cannot launch other agents (ship playbook step 3), so they stop at an open PR
with green CI and report it to the session that launched them, which writes the PR's
description. Only a coordinating session writes a `Coordinator-approved-head` line (DEC-175); an
author never adds, edits, or copies one. The Claude Code coordinating session is the merge
coordinator for both sides: it launches the independent review on a different model, relays
findings back to the author through the PR, and on PASS squash-merges (the reviewer may merge on
its behalf, as ship.md step 5 allows). Merges go in claim order, one at a time, so shared files
never race. Under DEC-175 the `merge` workflow may land two approved PRs close together; one that
a merge left conflicting is not mergeable, so the workflow skips it, and its author merges `main`
in and asks for a new approval of the new head. Nobody rebases, force-pushes, or closes a branch
they did not create; to ask for a review, a decision, or a rebase, comment on the PR or the claim
issue.

The exception is UI PRs (DEC-200): the Cursor coordinating session launches their independent
review on a different model and merges them itself, and the Claude Code session neither reviews nor
merges them.

## 6. Secrets and environments

Local secrets stay in `~/.config/mandate/paper.env` on the founder's machine; cloud agents use
the secrets of their own environment. Nothing is ever committed, and no agent connects to a live
account (AGENTS.md rule 8).

## 7. Handoff

At the end of every session, update the tracker's "Claims" table and your claim issues, so the
next session, on either side, starts from GitHub alone.
