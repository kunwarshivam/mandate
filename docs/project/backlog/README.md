# New backlog rows

Since [DEC-2373](../decisions/DEC-2373.md), a new backlog row, a story or a debt row, is one file
here, not a line added to [06-backlog-v1.md](../06-backlog-v1.md). Adding a row adds a file, so
two PRs adding rows never touch the same lines and never conflict.

## One file a row

- **Name:** `<story>-<slug>.md`, the story the row belongs to and a few words of it, for example
  `E21-30-quote-feed-resting-protection.md`.
- **Contents:** the row exactly as it would read in 06-backlog-v1.md: one Markdown bullet that
  opens `- **<story> ...**`, so the checks that look a story up (`check_planned.py`, the cell
  scenario's lane check) find it here as they would there.
- **Where it belongs:** the first line after the bullet may name the 06 section it folds into, as
  `<!-- section: E21 Operations and infrastructure -->`.

## Folding

The coordinator, or any lane in a docs-only PR, moves rows from here into 06-backlog-v1.md and
deletes their files in the same change, moving each row's text unchanged. `cargo xtask lint` lets
06 gain a row only when the change deletes the file here that holds it, and refuses any other row
added to it.

Editing a row already in 06, its status for instance, stays an edit of 06.
