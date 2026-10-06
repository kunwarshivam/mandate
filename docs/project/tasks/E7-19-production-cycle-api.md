# Task: E7-19 production cycle API

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. E7-19 corrects E7-7's one-shot assembly without weakening its safety checks.

## Story

- **Story:** E7-19 ([backlog](../06-backlog-v1.md#e7-alpaca-connector-and-recovery)).
- **Acceptance criteria (verbatim):** "As an owner, I want paper deployments to run through a
  production cycle API over my confirmed mandate and effective configuration, so that an
  instrument, model or deployment can change without a release and paper proves the path the
  product uses." *Accepted when:* the production cycle takes a validated deployment,
  authoritative account and market snapshots, and a registered `ModelOutput`; no production shell
  assembly selects an instrument, model, deployment identity, gate configuration or executor
  configuration; every effective input is content addressed in the journal; recorded paper tests
  and the manual Alpaca paper run use that same API with no synthetic model or test-only execution
  branch; and restart reconciliation sends no duplicate.
- **PRD / HLD / spec anchors:** PRD FR-3.1, FR-3.5, FR-3.7 and FR-4.3; HLD §5 and §6.B; mandate
  spec §6 and §8; trading-domain spec §9 to §12; journal spec §3, §5.2 and §8.
- **Decisions that apply:** DEC-07, DEC-79, DEC-97, DEC-133, DEC-138, DEC-157, DEC-466,
  DEC-470, DEC-471, [DEC-475](../decisions/DEC-475.md), and
  [DEC-484](../decisions/DEC-484.md).

## Scope

- **Reference cases that must move from pending to passing:** none. Existing mandate and
  trading-domain cases remain passing.
- **Invariants touched:** the mandate is the contract; model outputs are opinions, never orders;
  journal before acting; paper and live share one runtime; replay never resubmits; incomplete,
  stale or ambiguous input refuses.
- **Crates in scope:** `mandate-shell`, and `mandate-runtime` only if its existing
  `Input::ModelOutput` boundary cannot express a complete cycle without changing semantics.
- **Crates out of scope:** `web/`, model implementations, research, live connectors, OAuth, vault,
  approval channels and the control-plane services that will call the API.
- **New dependencies allowed:** none.
- **Safety-critical:** yes. Tests are written first against the public production API; the
  implementation follows without a test-only code path.
- **Size budget:** each review slice stays below 400 non-generated changed lines in
  safety-critical crates. Slices may stack, but each leaves the paper path runnable.

### Production input

The public input is typed and complete before execution starts:

1. opaque deployment identity and the confirmed mandate version;
2. content-addressed policy, gate, executor, calendar, fee, instrument and model-registry
   snapshots;
3. authoritative broker account, positions and open orders;
4. authoritative market, session, status, quote and liquidity snapshots;
5. a registered `ModelOutput` whose model, version, hash, instrument, time and expiry match the
   mandate and snapshots.

The model gateway creates item 5. The production cycle does not run a strategy or infer a signal.
The builder and gate remain the only code that sizes and permits an order.

### Migration slices

1. Add the typed production-cycle input and make the signal/model output caller-supplied. Keep the
   E7-7 adapter as a temporary caller.
2. Move deployment identity, instrument and model selection from constants into validated input.
3. Load gate and executor values from effective-dated, content-addressed configuration and remove
   the compile-time production defaults.
4. Move policy and model-registry snapshots into the validated input and complete journal
   `config_refs` and `artifact_refs`.
5. Make the Alpaca paper adapter call the production API, run the manual paper order and restart,
   then delete the E7-7-only assembly.

## Commands

```bash
cargo nextest run -p mandate-shell
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants
cargo xtask check
```

The manual command uses Alpaca paper credentials and the production paper adapter only after the
implementation, local checks and independent review pass. It still requires explicit confirmation
immediately before its one submission.

## Stop conditions

Stop and write a decision rather than continuing if:

- the API needs trading, sizing, pricing or gate logic in `mandate-shell`;
- a caller can construct a cycle without a confirmed mandate or complete configuration references;
- paper needs a branch the production cycle cannot take;
- a test requires a synthetic shipping model or a weakened E7-7 invariant;
- a new dependency is necessary;
- any live host, credential or order path becomes reachable.

## Definition of done

- [ ] The production shell contains no selected instrument, model, deployment identity, gate
      configuration or executor configuration.
- [ ] A model output mismatching any confirmed or registered field refuses before a journaled
      intent.
- [ ] Recorded paper transports exercise the public production API and every E7-7 fail-closed,
      write-ahead and restart invariant remains green.
- [ ] Every effective input used by a decision appears by content hash in its journal envelope.
- [ ] The Alpaca paper adapter calls the same API, one explicitly confirmed paper order is placed,
      the journal verifies, and restart sends no second order.
- [ ] No synthetic model, proof mode or test-only execution branch exists in production code.
- [ ] `cargo xtask check` is green with zero missed mutants.
- [ ] An independent different-model review approves every safety-critical implementation slice.
