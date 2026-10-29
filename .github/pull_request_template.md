## What and why

What the change does for the product, and the story ID if there is one.

## Dependencies and journal events

None, or the new dependency (with its `docs/dependencies.md` row) and the new event types.

## Safety-critical changes only

- [ ] Each invariant or spec clause touched has a named test
- [ ] No floats in money or quantity paths; no `unwrap`, `expect`, or panics on trading paths
- [ ] Inputs processed in journal order; replay gives identical outputs
- [ ] No secrets or trading content in logs or notifications

## Checks

- [ ] `cargo xtask check` green
