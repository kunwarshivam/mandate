- **E15-14 (Must, M8; SC; lane DR; after E15-13, with E21-29 and the data process)** As an owner,
  I want my deployed agent's runtime to evaluate its pinned quant models on each risk-clock tick
  and journal their outputs, so the order builder sizes from the pinned code's output in the
  running process, not only in the founder's paper run ([DEC-503](../decisions/DEC-503.md),
  [DEC-2383](../decisions/DEC-2383.md)). *Accepted when:* the runtime process, on a
  `ClockAdvanced` tick newer than any it has evaluated at, hands the core, for each quant model
  the deployed mandate version pins, `Input::Observation` (the closes as an artifact, `data_ref`)
  and then `Input::ModelOutput`, before `Input::Tick`; a refusal, a short or stale series, or an
  unreadable source hands no output (MI-10); a restart does not re-evaluate an `as_of` already
  journaled; exits, protective orders, kill switches and reconciliation never call the host
  (INF-13), and replay reads the records and never runs the host (INF-10); and the cell's scenario
  step 4 shows `ObservationRecorded`, then `ModelOutputRecorded` v2 on `agent:{ws}:demo-spy`.
<!-- section: E15 Signal models: LLM and fast models, scorecards -->
