# Autonomous Trading Agents Platform

A production service for deploying autonomous trading agents. Users connect their
brokerage or exchange accounts, describe an agent's goal and behavior, and deploy it. The
agent trades on its own within the limits its owner set, asks for approval when it is
unsure, and records every decision it makes.

It runs on managed infrastructure or on the customer's own edge / on-prem environment, and
supports multi-tenant organizations with SSO.

This repository is at the design stage.

## Documents

- [High-Level Design](docs/HLD.md): architecture, agent runtime, escalation flow,
  multi-tenancy, audit logging, billing, and open decisions.
