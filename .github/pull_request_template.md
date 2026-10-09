## Story

E?-? — <title>. Task brief: <link>. Decisions: DEC-NN.

## Spec clause → test

| Spec clause or invariant | Test name |
|---|---|
| | |

## Dependencies

None, or: name, purpose, alternatives, license, transitive count, and the `docs/dependencies.md` row.

## Journal events added

None, or the event types and their streams.

## Not done

What this change deliberately leaves out.

## Decisions needed

None, or the questions for the founder.

## Safety checklist (safety-critical changes)

- [ ] No floats in money or quantity paths
- [ ] No clock reads or randomness in the core
- [ ] No `unwrap`, `expect`, or panics on trading paths
- [ ] Ordering: inputs processed in journal order
- [ ] Replay: same inputs give identical outputs
- [ ] No secrets or trading content in logs or notifications
- [ ] No `x-planned` or `(planned: ...)` marker added to a value the code already serves, or a reviewer signed it off (DEC-683)

## Checks

- [ ] `cargo xtask check` green (summary below)
- [ ] Review agent report attached
