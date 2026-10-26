- **E7-19 debt: the owner's exit code display (no lane yet; found by lane DR on #2430's cell run)**
  As an owner, I want `mandate agent exit --show-code` to show a code only where one is checked, so
  that the exit's code says what it confirms. Found: `agent exit --show-code` prints a code bound to
  the control stream's head (crates/mandate-cli/src/agent.rs `code_at`, DEC-279 item 1), but `agent
  exit` never requires it: `needs_code` covers resume, Stop and acknowledgments, not `Exit`, so a
  typed code that is not the shown one, or none, commits the exit all the same. That is by design:
  an owner's exit reduces risk, which never needs approval (AGENTS.md rules 2 and 13), and is paced
  only by participation caps. On #2430's cell run the shown code was for control head 12 (the role
  provisioner's records moved it) and the typed one for head 10, and the exit committed. Two
  readings, for whoever takes this row: (a) the code is rule 13's displayed-bid confirmation for
  selling equities outside the regular session, so it is bound and checked in exactly that case and
  in no other, where a wrong code refuses that out-of-session sale only; or (b) the code is leftover
  output, and `agent exit` prints none. *Accepted when:* one reading is decided (a decision under
  DEC-176, or the founder's if it touches rule 13's wording), the CLI and its tests follow it, and
  the cell scenario, which commits the exit with no code since #2454, still passes.
<!-- section: E7 Alpaca connector and recovery -->
