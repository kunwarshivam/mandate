- **E21-64e (Proposed; lane RN)** As the founder, I want the CI runners in a VPC of their own, so
  that in a shared DigitalOcean team (DEC-2074) a compromised runner host does not sit on the
  staging and production clusters' network, whose node firewall admits its whole VPC range.
  *Accepted when:* `modules/ci-runners` creates a `digitalocean_vpc` with a range no other VPC of
  the account uses, the droplets set its `vpc_uuid`, a test pins both, and the runbook says the
  apply replaces the droplets.
