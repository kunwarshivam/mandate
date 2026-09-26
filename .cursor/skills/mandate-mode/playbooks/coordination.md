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
| `claude-code` | E3-3, E4-1, E4-2, E17-0 (spike), the DEC-97 and DEC-98 spec rewrite (Track C) | `mandate-accounting`, `mandate-refcases`, `mandate-sim`, `python/research_spike/`, `docs/specs/mandate.md`, `schemas/`, `reference/` |
| `cursor` | E2-4, E2-2, E5-2, E5-3 | `mandate-marketdata`, `mandate-cli`, `mandate-journal`, `mandate-journal-pg`, `.cursor/install.sh` and CI environment work |

Shared, owned by no one: `mandate-num`, `mandate-time`, `mandate-canon`, `xtask`. A change there
needs its own claim issue, stays minimal, and says which story needs it.

## 3. Reserve identifiers before you use them

Decision IDs (`DEC-`), open decisions (`OD-`), ADR numbers, epic and story IDs, and PR sequence
names collide silently between sessions. Before using one:

1. Find the highest number across `main`, every open PR, and every open claim issue
   (`gh pr list --search DEC-`, `gh issue list --label claim`).
2. Take the next one and comment `DEC-<n> reserved` (or `OD-<n>`, `ADR-<n>`) on your claim
   issue before the first commit that uses it.

## 4. Shared files

The work tracker, the decision log, the backlog, the feature map, and `status.toml` are edited by
everyone. In each: add or change only the rows for your own stories; never rewrite another
coordinator's rows; append rather than reorder. Rebase onto `main` immediately before merging
and resolve conflicts by keeping both sides.

## 5. Merge only your own

The owning coordinator merges its PRs after green CI and its independent review (the ship
playbook). Never merge, rebase, force-push, or close another coordinator's PR or branch. To ask
the other side for a review or a decision, comment on its claim issue.

## 6. Secrets and environments

Local secrets stay in `~/.config/mandate/paper.env` on the founder's machine; cloud agents use
the secrets of their own environment. Nothing is ever committed, and no agent connects to a live
account (AGENTS.md rule 8).

## 7. Handoff

At the end of every session, update the tracker's "Claims" table and your claim issues, so the
next session, on either side, starts from GitHub alone.
