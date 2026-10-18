# The node firewall admits no public source (DEC-2012 items 4 and 8), checked on the plan against
# a mocked provider.

mock_provider "digitalocean" {}

variables {
  name     = "mandate-staging"
  region   = "nyc3"
  ip_range = "10.110.0.0/20"
}

run "firewall_admits_only_the_vpc" {
  command = plan

  assert {
    condition = alltrue(flatten([
      for rule in digitalocean_firewall.nodes.inbound_rule :
      [for source in rule.source_addresses : source == "10.110.0.0/20"]
    ]))
    error_message = "An inbound rule admits a source other than the VPC's own range."
  }

  assert {
    condition = alltrue([
      for rule in digitalocean_firewall.nodes.inbound_rule :
      length(rule.source_addresses) > 0 && length(coalesce(rule.source_tags, [])) == 0 && length(coalesce(rule.source_load_balancer_uids, [])) == 0
    ])
    error_message = "Every inbound rule names the VPC's range and nothing else."
  }

  assert {
    condition     = length(digitalocean_firewall.nodes.inbound_rule) == 3
    error_message = "The node firewall has an inbound rule beyond the VPC's tcp, udp and icmp."
  }

  assert {
    condition     = digitalocean_firewall.nodes.tags == toset(["mandate-staging-nodes"])
    error_message = "The firewall applies to the cluster's node tag."
  }
}

run "refuses_a_public_vpc_range" {
  command = plan

  variables {
    ip_range = "203.0.113.0/24"
  }

  expect_failures = [var.ip_range]
}
