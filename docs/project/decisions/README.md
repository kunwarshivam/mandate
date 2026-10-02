# Decisions, one file each

From DEC-344 on, every decision is its own file here, `DEC-<n>.md` ([DEC-344](DEC-344.md)).
DEC-01 to DEC-302 stay where they were written, as rows of the
[decision log](../04-decision-log.md), which takes no new decision rows. Both are binding in the
same way (`AGENTS.md` rule 9).

## Why files

Every PR used to append its row to the same two tables of one file, so every merge left every
other open PR conflicting. Two PRs that add two different files never conflict. Two PRs that take
the same number add the same path, which git refuses to merge, so a collision is loud rather than
silent.

## Writing a decision

1. **Take a number.** Use the range your dispatch or claim gives you. With none, fetch `main` and
   take the next integer after the highest of: the files here, the decision log's rows, and its
   Reserved identifiers table. Every number through DEC-343 is spoken for, so in practice the next
   one is above the highest file here.
2. **Reserve it in your first commit** by adding `DEC-<n>.md` with the status `Reserved`, who holds
   it, and one line on what it is for. Reserve only what you will use: a number is never reused.
3. **Fill it in** in the PR that makes the decision, and set the status.
4. **Change nothing else here.** A PR edits only its own decision files. To change an accepted
   decision, write a new one that supersedes it, and edit the old decision's status line only:
   its file's, or, for DEC-01 to DEC-302, the Status cell of its row in the
   [decision log](../04-decision-log.md), which is the one edit that table still takes.

A reservation that reached `main` and is then abandoned keeps its file, with the status
`Released`, and its number is never reused. One whose PR closed without merging leaves nothing
behind, and the number simply falls free for the next lane.

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

The five statuses: **Reserved**, the number is taken and the decision is not written yet;
**Proposed**, recommended and waiting for the founder; **Accepted**, binding (`AGENTS.md` rule 9),
which an agent may set itself only for the decisions DEC-79 and DEC-176 leave to agents;
**Superseded by DEC-<m>**, replaced by a later decision; and **Released**, a reservation that was
merged and then abandoned. **Accepted**, **Proposed** and **Superseded** mean what they mean in
the decision log, which also has **Open** for a question not yet decided; an open question is an
`OD-`, not a decision file.

## Numbers below DEC-344

DEC-303 to DEC-343 were handed out before this directory existed. One of them that is not yet on
`main` is written as a file here too; a PR that already carries it as a decision-log row moves the
row's text into a file the next time it merges `main`. A number reserved in the log's table and
never written (DEC-295, for example) keeps its row there and gets its file here when it is used.
