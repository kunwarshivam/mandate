# Chat with an agent (DEC-2293)

- **Spec:** DEC-2293 (the founder's rulings on the agent's chat), agent harness spec §15 and
  §15.13, workspace API spec §4.6.2 (Home, a thread, Stop, goals, models, settings and the setup
  thread), journal spec §9.39 (the chat's seven control-stream records).
- **Code:** `mandate-chat`, `crates/mandate-chat/` (the pinned chat entry and its tools per kind,
  `src/entry.rs`, and the turn loop over the model gateway, `src/engine.rs`: every model call is
  journaled before its tools run, a tool the kind does not offer never runs, and Stop and the daily
  spending cap end a run before its next call); in `mandate-api-server`, `src/chat.rs` (the routes,
  the thread read from the journal, the settings and the models read from the gateway's registry),
  `src/chat_run.rs` (a run on its own thread: reads of the agent's own records, and actions only
  through the routes the web app uses, under the owner's session), `src/chat_watch.rs` (the agent
  messages first: fills and approvals posted without a model, each once, and the daily note);
  in `web/`, `src/api/chat.ts` (the decoders and the typed cards), `src/lib/fixture-chat.ts` (the
  recorded demo's chat, which reads nothing the owner writes), and
  `src/components/agent-chat/` (Home, the agent's thread, its Details drawer with the owner's exit,
  the setup thread and its confirm card, `/new?version=` for a version made outside the chat, and
  each agent's chat settings).
- **Tests:** `crates/mandate-chat/tests/chat_loop.rs` (the entry registers and each kind offers
  its own tools, a run journals each call before its tools, a tool not offered is refused, Stop and
  the cap end a run before any call, a halt on journaling stops before any tool);
  `crates/mandate-api-server/tests/chat_http.rs` (a message's run against a fake gatewayd, no
  gateway, a blank or unknown agent, a pause committed through the owner's command, the daily cap,
  and the watcher's fill and approval posted once, silent without an owner, its daily note's marker
  hidden); `web/src/api/chat.test.ts`, `web/src/components/agent-chat/agent-chat.test.tsx`,
  `web/e2e/golden-path.spec.ts`, `web/e2e-api/api-source.spec.ts`, and the cell's
  `web/e2e-cell/trade-loop.spec.ts` (the approval asked and the fill said in the agent's thread,
  the exit from Details).
- **Not yet:** push notifications for the agent's first messages (the web-push pipeline, E8-14,
  is not live), and a chat model in the local cell, whose model gateway serves none.
- **Run:** `cargo nextest run -p mandate-chat -p mandate-api-server`, and in `web/`,
  `npx vitest run src/api/chat.test.ts src/components/agent-chat`.
