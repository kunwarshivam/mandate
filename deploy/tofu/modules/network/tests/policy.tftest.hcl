# The node firewall admits no public address (DEC-2012 items 4 and 8), checked on the plan against
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
      [for address in rule.source_addresses : address == "10.110.0.0/20"]
    ]))
    error_message = "An inbound rule admits an address other than the VPC's own range."
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

# DEC-2054: the VPC is named for its region, so it never takes the name of a VPC the account
# already holds elsewhere.
run "the_vpc_is_named_for_its_region" {
  command = plan

  assert {
    condition     = digitalocean_vpc.west.name == "mandate-staging-nyc3" && digitalocean_vpc.west.region == "nyc3"
    error_message = "The VPC is named for its environment and region."
  }
}

# DEC-2064 item 7: the default egress is unchanged for staging and production, and a cell may
# narrow it.
run "the_default_egress_is_unchanged" {
  command = plan

  assert {
    condition     = length(digitalocean_firewall.nodes.outbound_rule) == 8
    error_message = "The default keeps seven outbound ports and the VPC's icmp."
  }
}

run "a_cell_may_drop_plain_web" {
  command = plan

  variables {
    egress = [
      { protocol = "tcp", port_range = "443" },
      { protocol = "udp", port_range = "123" },
    ]
  }

  assert {
    condition     = length(digitalocean_firewall.nodes.outbound_rule) == 3 && !anytrue([for rule in digitalocean_firewall.nodes.outbound_rule : rule.port_range == "80"])
    error_message = "The narrowed egress opens only its own ports."
  }
}

run "refuses_an_egress_range" {
  command = plan

  variables {
    egress = [{ protocol = "tcp", port_range = "1-65535" }]
  }

  expect_failures = [var.egress]
}

run "refuses_a_port_past_65535" {
  command = plan

  variables {
    egress = [{ protocol = "tcp", port_range = "99999" }]
  }

  expect_failures = [var.egress]
}
