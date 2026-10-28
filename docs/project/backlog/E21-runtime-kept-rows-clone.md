- **E21-runtime-kept-rows-clone (Proposed; debt; lane D1)** As an operator, I want the runtime
  process's kept control records (DEC-2852) handed to the ports that read them by reference rather
  than cloned at every pass, so that a long-lived runtime's per-pass memory copies stop growing with
  the control stream. The agent stream's part is done: the runtime keeps of it only its thesis
  records and journaled outputs (DEC-2856).
  *Accepted when:* a soak test shows a pass's allocation bounded by the records it read.
