- **E7-4b founder connection step-up (Proposed, M10; SC)** As the founder, I want to acknowledge a
  hold on my own live connection with `cli_confirm`, as journal spec §9.28 check 3 allows there
  (DEC-529 item 3, DEC-923, DEC-2523), so that the CLI serves my live account as it serves paper.
  The executor process cannot yet tell the founder's own connection from another, so it hands every
  live acknowledgment with `passkey` alone and refuses a live `cli_confirm` `step_up_method`
  ([DEC-2838](../decisions/DEC-2838.md) item 5). *Accepted when:* `mandate-executord` identifies the
  founder's own connection from the connection's own journaled facts, never from configuration or
  an environment variable, and adds `cli_confirm` to `methods` for that connection alone, with a test
  that another live connection still refuses it.
<!-- section: E7 Alpaca connector and recovery -->
