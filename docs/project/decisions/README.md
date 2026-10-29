# Decisions

A decision starts as its own file here, `DEC-<n>.md`, reserving its number
([DEC-344](earlier-303-599.md#dec-344)). Once it is settled and nothing under `crates/`, `web/`,
`docs/specs/`, `deploy/cell/charts/`, `deploy/cell/model-gateway/`, `schemas/` or `fixtures/`
cites it, it folds into its category's file as a `## DEC-<n>:` section under a stable
`<a id="dec-<n>">` anchor, and its own file is deleted ([DEC-2650](DEC-2650.md)). DEC-01 to
DEC-302 stay where they were written, as rows of the
[decision log](../04-decision-log.md), which takes no new decision rows. All of it is binding in
the same way (`AGENTS.md` rule 9).

| Range | Category file |
|---|---|
| DEC-2000 to DEC-2099 | [infrastructure.md](infrastructure.md) |
| DEC-2100 to DEC-2199 | [workspace.md](workspace.md) |
| DEC-2200 to DEC-2299 | [intelligence.md](intelligence.md) |
| DEC-2300 to DEC-2399, DEC-2800 to DEC-2899 | [trading-core.md](trading-core.md) |
| DEC-2400 to DEC-2499 | [web.md](web.md) |
| DEC-2500 to DEC-2599 | [notifications.md](notifications.md) |
| DEC-2600 to DEC-2699 | [process.md](process.md) |
| DEC-2700 to DEC-2799 | control-plane.md (empty so far) |
| DEC-303 to DEC-599 | [earlier-303-599.md](earlier-303-599.md) |
| DEC-600 to DEC-899 | [earlier-600-899.md](earlier-600-899.md) |
| DEC-900 to DEC-1999 | [earlier-900-1999.md](earlier-900-1999.md) |

Each category file opens with a one-page summary of the rules still in force, then holds its
decisions verbatim. A decision that stays a file — reserved or proposed, Released, or cited from
a forbidden path — keeps its own `DEC-<n>.md` until that changes.

## Why files for the living decisions

A number is reserved by adding the file that takes it: two PRs that take the same number add the
same path, which git refuses to merge, so a collision is loud rather than silent. The settled
decisions then live one category file each, so reading a category does not open a hundred files,
and a link to a decision is a link to its category file's `#dec-<n>` anchor.

## When to write one

Only for a founder ruling or an architecture choice (the founder, 2026-10-29). An ordinary
engineering choice goes in its PR description instead. Decisions that nothing in the code, specs or
plans cited any more were removed on 2026-10-29; git history keeps them.

## Writing a decision

1. **Take a number** from your category's range ([Numbers by category](#numbers-by-category)):
   fetch `main` and take the next integer in that range after the highest decision the category
   holds — its own file, or a section of its category file.
2. **Reserve it in your first commit** by adding `DEC-<n>.md` with the status `Reserved`, who holds
   it, and one line on what it is for. The file is what protects the number: citing a number in a
   PR description or a commit message reserves nothing. Reserve only what you will use.
3. **Fill it in** in the PR that makes the decision, and set the status.
4. **Change nothing else here.** A PR edits only its own decision files. To change an accepted
   decision, write a new one that supersedes it, and edit only the old decision's status: the
   status line of its file, the Status cell of its section in its category file, or for DEC-01 to
   DEC-302 the Status cell of its row in the decision log, which is the one edit that table still
   takes.

A number that reached `main` is never reused: a reservation abandoned after it merged keeps its
file, with the status `Released`. A reservation in a PR that is closed without merging leaves
nothing on `main`, so the number is free and the next lane takes it.

## Numbers by category

From [DEC-2600](process.md#dec-2600) on, each category of work has its own range of 100 numbers, so lanes
in different categories never race for the same next number. Within its category, a lane takes the
next free number and reserves it with its file, as above; a lane that needs several reserves each
with its own file. Two lanes of one category that take the same number add the same path, which git
refuses to merge, and `.github/scripts/decision-numbers.sh` (run by `cargo xtask decisions`, the
lint job, and the docs-only path's `docs-checks.sh`) refuses two files with one number anywhere else in the tree, a leading zero or a
suffix included, and a heading whose number differs from its file name's.

| Range | Category |
|---|---|
| DEC-2000 to DEC-2099 | Infrastructure and the cell |
| DEC-2100 to DEC-2199 | The workspace API and identity |
| DEC-2200 to DEC-2299 | Intelligence: the model gateway, research, data, backtests |
| DEC-2300 to DEC-2399 | The trading core, R0, and live |
| DEC-2400 to DEC-2499 | The web app |
| DEC-2500 to DEC-2599 | Notifications and approvals |
| DEC-2600 to DEC-2699 | Process and CI |
| DEC-2700 to DEC-2799 | The control plane |
| DEC-2800 to DEC-2899 | The trading core, R0, and live: its second range, once DEC-2300 to DEC-2399 ran out |

Work that fits two categories takes the one whose crates or documents it changes most. When a
category's range runs out, a PR that edits only this table gives it the spare range, and the next
one after that is DEC-2900 to DEC-2999. Numbers from DEC-996 to DEC-1999 are no longer handed out:
the interim blocks below stay with the lanes that hold them, and a number an open PR reserved there
before this table keeps its reservation.

### The interim blocks (history)

Before the ranges, the coordinator gave each lane a block of ten (five for W1 and W2) after lanes
kept reserving the same next number. They were valid until the lane's story ended.

| Lane | Block | Lane | Block |
|---|---|---|---|
| P | DEC-1010 to DEC-1019 | ID1 | DEC-1100 to DEC-1109 |
| A1 | DEC-1020 to DEC-1029 | I1 | DEC-1110 to DEC-1119 |
| V | DEC-1030 to DEC-1039 | I2 | DEC-1120 to DEC-1129 |
| K1/C | DEC-1040 to DEC-1049 | I3 | DEC-1130 to DEC-1139 |
| S | DEC-1050 to DEC-1059 | I4 | DEC-1140 to DEC-1149 |
| R | DEC-1060 to DEC-1069 | R0 | DEC-1150 to DEC-1159 |
| O | DEC-1070 to DEC-1079 | L3 | DEC-1160 to DEC-1169 |
| G/M1 | DEC-1080 to DEC-1089 | D1 | DEC-1170 to DEC-1179 |
| W1 | DEC-1090 to DEC-1094 | K0 | DEC-1180 to DEC-1189 |
| W2 | DEC-1095 to DEC-1099 | CI-R | DEC-1190 to DEC-1199 |
| | | M2 | DEC-1200 to DEC-1209 |

## Format

```markdown
# DEC-<n>: <the decision in one line>

| | |
|---|---|
| **Status** | Reserved / Proposed / Accepted / Superseded by DEC-<m> / Released |
| **Date** | YYYY-MM-DD (UTC) |
| **Decided by** | the founder, or the agent and the delegation it decided under (DEC-79, DEC-176) |
| **Story or claim** | story ID, claim issue, PR |

## Decision

What was decided. Number the items when there are several, so they can be cited as
"DEC-<n> item 3".

## Rationale

## Alternatives considered
```

The status values:

- **Reserved:** the number is held and the decision is not written yet.
- **Proposed:** recommended and waiting for the founder. Work continues with the most conservative
  option.
- **Accepted:** binding. An agent may set it itself only for decisions DEC-79 and DEC-176 leave to
  agents.
- **Superseded by DEC-<m>:** replaced by a later decision, which says what changed.
- **Released:** a reservation that reached `main` and was then abandoned.

A question nobody has decided yet is not a decision file: it stays an `OD-` row in the decision
log's Open decisions table.

## Numbers below DEC-344

DEC-303 to DEC-343 were handed out before this directory existed. One of them that is not yet on
`main` is written as a file here too; a PR that already carries it as a decision-log row moves the
row's text into a file the next time it merges `main`. A number reserved in the log's table and
never written (DEC-295, for example) keeps its row there and gets its file here when it is used.
