# Security Policy

Mandate is a platform for autonomous trading agents that act on real brokerage accounts (paper
environments only today). Its safety-critical surface — the risk gate, the mandate validation,
the journal, the executor, the broker connectors, and credential handling — is the code this
policy protects.

## Supported versions

| Version | Supported |
|---|---|
| `main` (pre-release) | yes — security fixes land on `main` first |

There are no tagged releases yet; reports against `main` are in scope.

## Reporting a vulnerability

**Please do not open a public issue for a security report.**

Use GitHub's **private vulnerability reporting** on this repository (Security → Report a
vulnerability), or contact the repository owner through the
[GitHub profile](https://github.com/kunwarshivam) (profile contact link). Please include:

- a description of the issue and its impact,
- the exact revision (`git rev-parse HEAD`) and how to reproduce it,
- a proof of concept, if you have one.

We will acknowledge reports promptly and keep reporters informed of the fix's progress. We ask
for responsible disclosure and will credit reporters in the fix's description unless asked not
to.

## Scope

In particular, these areas are security-sensitive and reports about them are welcome:

- **The risk gate and mandate enforcement** — any path by which an agent could act outside its
  mandate's limits, autonomy rules, or the US account rules (AGENTS.md rules 1, 10, 12, 13).
- **The journal** — any way to write an event that does not verify, or to make a recorded
  decision unverifiable (rules 4, 5).
- **Credential handling** — anything that moves broker credentials, tokens, or secrets out of
  the vault, into logs, or into committed files (rules 7, 8). No contributor ever needs real
  credentials to work on Mandate; if a change appears to require them, that change is wrong.
- **Authentication, step-up, and tenant isolation.**
- **Notification payloads** — anything that leaks order details, positions, or mandate content
  beyond the workspace deployment (rule 6).
- **The web application's** session handling and tenant boundaries.

The [threat model](docs/security/threat-model.md) is the fuller map of adversaries and surfaces.

## Out of scope

- Losses on your own brokerage account from your own configuration choices.
- Anything requiring access to live trading credentials — Mandate never uses them, and reports
  that assume a live account are not actionable.
- Volumetric attacks on the repository's GitHub presence.
