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

| Skill | Upstream path | Commit | License |
|---|---|---|---|
| `impeccable` (skill 4.4.0) | [pbakaus/impeccable](https://github.com/pbakaus/impeccable), `.cursor/skills/impeccable/` (the repository's Cursor build of `skill/`) | `9d715cc4f5564a990ca8345abfdd5df6dc9b41c8` (2026-09-24) | Apache-2.0: `LICENSE-impeccable`, with its notices in `NOTICE-impeccable.md` |
| `emil-design-eng`, `review-animations`, `prototype`, `apple-design` | [emilkowalski/skills](https://github.com/emilkowalski/skills), `skills/<name>/` | `d16ebe60d09a5ba2afcb7054ede9d0a10c9f6128` (2026-09-24) | MIT, Emil Kowalski: `LICENSE-emilkowalski-skills` |
| `baseline-ui`, `fixing-accessibility`, `fixing-motion-performance` | [ibelick/ui-skills](https://github.com/ibelick/ui-skills), `skills/<name>/` | `f9515cb90e9d1ed1eee9384f79f1e0ac6c26825e` (2026-09-26) | MIT, Julien Thibeaut: `LICENSE-ui-skills` |
| `web-design-guidelines` | [vercel-labs/agent-skills](https://github.com/vercel-labs/agent-skills), `skills/web-design-guidelines/` | `063bee94c3f4df8453406c830b0a7df0f2860278` (2026-08-28) | MIT, declared in the upstream README; the repository has no LICENSE file: `LICENSE-vercel-agent-skills` |
| The rules `web-design-guidelines` reads | [vercel-labs/web-interface-guidelines](https://github.com/vercel-labs/web-interface-guidelines), `command.md`, copied to `web-interface-guidelines/command.md` here | `e3d624baaf29dc1fc645aff3e38f03e564d2d6b1` (2026-08-17) | MIT, Vercel Labs: `LICENSE-web-interface-guidelines` |

When to use which:

- **Direction, critique, audit, and polish:** `impeccable`. Mandate's screens are its Operate mode
  (app UI, where scanability outranks expression).
- **Motion:** `emil-design-eng` for how an interaction should move, and `review-animations` to
  review motion code against its standards. `apple-design` for platform feel.
- **Comparing directions:** `prototype`, which builds several variants behind a picker.
- **Review passes before a UI PR:** `baseline-ui`, `fixing-accessibility`,
  `fixing-motion-performance`, and `web-design-guidelines`.

`web-design-guidelines` tells the agent to fetch its rules from GitHub. Read the vendored copy,
`.cursor/third_party/web-interface-guidelines/command.md`, instead: cloud agents may run behind
restricted egress, and the pinned copy is the one reviewed here.

`impeccable`'s launcher (`scripts/impeccable`) downloads its engine binary from the project's GitHub
releases on first run and checks it against the published SHA-256. The engine has telemetry: set
`IMPECCABLE_NO_TELEMETRY=1` and `DO_NOT_TRACK=1` before running it, as `.github/workflows/web.yml`
does for the detector.

Where these skills conflict with the
[product-experience brief](../../docs/product/09-product-experience.md), `AGENTS.md`, or DEC-200,
those win. In particular:

- **no gradients** (founder, 2026-09-28): flat colour, including no gradient text, gradient masks,
  or gradient fades at scroll edges, and no generic AI aesthetics;
- the stack is DEC-200's: shadcn/ui's primitives are the project's primitives, whatever a skill
  prefers (for example `baseline-ui`'s Base UI);
- no optimistic interface for anything that creates an order (brief §5, rule 5);
- no third-party analytics, session replay, or telemetry (DEC-200), and no cache of approval,
  position, or mandate content (brief §5, rule 6);
- no persuasion: Approve and Skip carry equal weight, in motion as in layout (P3, PX-10);
- generating images or sound through a paid API is spending, which only the founder approves
  (DEC-79).

To update, copy the directories again from a newer commit, record the commit here, and review the
diff like any other change. Do not edit vendored files in place; put repository-specific rules in
`mandate-mode` or `AGENTS.md`, which take precedence.
