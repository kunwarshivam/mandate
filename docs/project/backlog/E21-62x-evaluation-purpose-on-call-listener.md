# E21-62 follow-up: the call listener refuses purpose `evaluation`

- **E21-62x (Proposed; safety; spend)** As an owner, I want only the model-gateway service's own
  gate and start probe to make `evaluation` calls, so that a caller cannot dodge the daily
  research cap or its per-agent attribution by naming that purpose. Inference spec §4.3 says the
  call listener refuses `evaluation`, but nothing does yet: the TCP listener hands every request
  to the gateway, and `Gateway::call` (`mandate-gateway` `call.rs`) checks only that an agent is
  named exactly when the purpose names one (`Purpose::names_agent`). Any client that reaches the
  listener can send `evaluation` with no agent, which bills to the workspace and deployment caps
  with no daily research cap and no agent. The gate's own calls (DEC-2294) and the start's
  identity probe go to the gateway directly, not over the listener, so refusing `evaluation` there
  takes nothing from them.
  *Accepted when:* the call listener refuses an `evaluation` request before any reservation or
  record, with a code the wire and the journal spec admit; the gate and the identity probe still
  make `evaluation` calls; and a test over a real loopback connection shows an `evaluation`
  request refused, nothing reserved, and nothing sent.
<!-- section: E21 Operations and infrastructure -->
