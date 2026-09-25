# Vendored agent skills

These skills under `.cursor/skills/` are copied unchanged from
[cursor/plugins](https://github.com/cursor/plugins) at commit
`14253dfddd9fbc701554c2f5aee00c5594f4f050` (2026-09-25). They are vendored rather than enabled as
plugins because project-scoped plugins did not load for cloud agents, and because pinning keeps
their text reviewable (DEC-78).

| Skill | Upstream path | License |
|---|---|---|
| `how`, `why`, `tdd`, `blast-radius`, `interrogate`, `unslop`, `technical-writing` | `pstack/skills/<name>/` (pstack 0.15.5) | MIT, Lauren Tan: `LICENSE-pstack` |
| `deslop` | `cursor-team-kit/skills/deslop/` | MIT, Cursor: `LICENSE-cursor-team-kit` |

To update, copy the directories again from a newer commit, record the commit here, and review the
diff like any other change. Do not edit vendored files in place; put repository-specific rules in
`mandate-mode` or `AGENTS.md`, which take precedence.
