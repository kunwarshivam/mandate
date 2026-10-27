# E21-62 follow-up: an evaluation call has a journal record of its own

- **E21-62w (Proposed; records)** As an owner, I want every model call the platform makes recorded
  in the journal, so that the evaluation that offered my agent's model is as auditable as my
  agent's own calls. Today an `evaluation` call (DEC-2216) has no agent stream, journal spec §9.19
  admits no `evaluation` call record, and its record lives only in the gate's report artifact;
  its spend is journaled on the meter streams.
  *Accepted when:* the journal spec admits an `evaluation` call record on a stream the gateway
  service writes alone; the gate and the start's identity probe append it before use (INF-10); and
  a test replays a gate from the journal to its report's call list.
<!-- section: E21 Operations and infrastructure -->
