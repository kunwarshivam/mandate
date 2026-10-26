# E21-54 follow-up: one deployment manager over several workspaces

- **E21-54g (Proposed, M8; SC; lane GW)** As an operator, I want one deployment manager to serve
  every workspace a cell hosts, so that a hosted cell with several workspaces does not need one
  manager Deployment, ServiceAccount and Role for each. Today a manager reads one workspace's control
  stream (DEC-1172), and the local cell runs a second manager for the research agent's workspace
  (DEC-2070). The change is to `mandate-deploy-manager`, a safety-critical crate: its slots, pod
  names, admission policy and kill-switch scope are per workspace, with tests first.
