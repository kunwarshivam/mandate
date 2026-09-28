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

## Design skills for the web UI (DEC-200)

Copied unchanged, whole skill folders, for work under `web/`:

| Skill | Upstream | Commit | License |
|---|---|---|---|
| `frontend-design` | [anthropics/skills](https://github.com/anthropics/skills), `skills/frontend-design/` | `33375500bcea98d610eb30ce10ac4e59b89c390d` (2026-09-24) | Apache-2.0, Anthropic: `LICENSE-anthropic-skills` (also `LICENSE.txt` in the skill folder) |
| `design-engineering` | [AgentsORG/design-engineering](https://github.com/AgentsORG/design-engineering), `skills/design-engineering/` (version 2.4.0) | `81805dc89d40889639a95502bfb578a098266dc8` (2026-09-05) | MIT, HKTITAN: `LICENSE-design-engineering` |

`design-engineering` links to files outside its folder (`SOUL.md`, the repository's `agents/`),
which are not vendored; those links do not resolve here. Where these skills conflict with the
[product-experience brief](../../docs/product/09-product-experience.md) or `AGENTS.md`, the brief
and `AGENTS.md` win. In particular:

- no optimistic interface for anything that creates an order (brief §5, rule 5);
- no third-party analytics, session replay, or telemetry (DEC-200), and no cache of approval,
  position, or mandate content (brief §5, rule 6);
- no persuasion: Approve and Skip carry equal weight, in motion as in layout (P3, PX-10);
- sound generation through ElevenLabs or any other paid service is spending, which only the founder
  approves (DEC-79).

To update, copy the directories again from a newer commit, record the commit here, and review the
diff like any other change. Do not edit vendored files in place; put repository-specific rules in
`mandate-mode` or `AGENTS.md`, which take precedence.
