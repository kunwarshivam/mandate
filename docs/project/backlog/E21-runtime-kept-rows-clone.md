- **E21-runtime-kept-rows-clone (Proposed; debt; lane D1)** As an operator, I want the runtime
  process's kept control and agent records (DEC-2852) handed to the ports that read them by
  reference rather than cloned at every pass, and the agent stream's models and research reads
  folded from what follows its head (the models port's replay), so that a long-lived runtime's
  per-pass memory copies and its 256 MiB footprint stop growing with the agent stream.
  *Accepted when:* a soak test shows a pass's allocation bounded by the records it read, and the
  kept agent stream is bounded by what its ports fold from it.
