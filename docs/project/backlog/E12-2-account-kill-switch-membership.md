- **E12-2a (Proposed, M7; L5 or L1)** As an owner reading an agent's timeline, I want the account
  stream's `KillSwitchActivated` shown when it changed what the agent may do, so that the timeline
  hides no kill switch that stopped the agent. The account stream records the executor's switch at
  its agent and account scopes since journal spec v0.90 ([DEC-2139](../decisions/DEC-2139.md)), but
  DEC-764 item 2 and workspace API §4.8.1 name only the connection- and workspace-scope switch as
  account-wide, and DEC-777 left the agent-scope switch to "its own decision, made once the journal
  closes that payload". Until that decision both are served only through their links, which
  neither carries (the switch's `causation_id` is its account-stream `AgentModeApplied`).
  *Accepted when:* a decision says which scopes are the agent's (an account-scope switch changes
  every agent's mode; an agent-scope switch names the agent as `subject`), workspace API §4.8.1
  says so, and the timeline serves them with cases served and not served.
<!-- section: E12 Audit explorer -->
