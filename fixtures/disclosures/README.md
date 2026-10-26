# Disclosure documents

Each file is one disclosure an owner accepts, in the form the workspace stores it. Its version is
the SHA-256 of its canonical JSON (journal spec §9.2 `DisclosureAccepted`, `version`), so a changed
word is a new version.

| File | Document | Status |
|---|---|---|
| `go_live_risk.json` | The risks an owner reads on the go-live screen before an agent trades | Placeholder, pending counsel — not for real-money use (DEC-79, DEC-2134) |

Agents never write disclosure text as final (DEC-79). `mandate disclosure accept` reads these files
and records the owner's acceptance with step-up; `web/` shows the same text and tests that it
matches.
