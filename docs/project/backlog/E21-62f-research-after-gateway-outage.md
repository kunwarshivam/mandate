# E21-62 follow-up: research resumes after a model-gateway outage

- **E21-62f (Proposed, M8; SC; lane GW)** As an owner, I want my agent's research to resume once
  the model-gateway service is back, without restarting its runtime. Today a `gateway_unreachable`
  is a refusal, and the core withdraws a refused call's pin for the process's life (DEC-2397,
  DEC-2261), so one outage stops research until the runtime restarts. The change is to
  `mandate-runtime`'s research state: an outage refusal holds the pin for a bounded back-off rather
  than withdrawing it, never retries the lost call, and journals each hold. Tests first.
