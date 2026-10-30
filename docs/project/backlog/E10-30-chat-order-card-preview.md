- **E10-30 (from DEC-2859 item 7): the chat's order card shows the builder's sized quantity and
  limit.** The chat's `confirm_order` card shows the quantity the owner asked for, the last trusted
  daily close and the estimate at it, and says the builder may clip, because no route sizes an
  order without submitting it: the builder runs in the agent runtime on the journaled facts. The
  founder asked for the quantity after the builder sizes and clips it, the limit price and the
  estimated cost. *Accepted when:* a preview the runtime or a dry-run route (workspace API §4.6,
  DEC-190) answers from the same facts the builder clips on gives the card the clipped quantity,
  the limit price, each clip and the estimate at the limit, placing nothing and asking no one.
<!-- section: E10 Workspace services -->
