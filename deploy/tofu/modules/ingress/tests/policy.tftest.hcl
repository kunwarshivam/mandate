# A cell's per-workspace ingress (DEC-2064 item 5), planned against a mocked provider: each
# workspace's host routes only to its own namespace's API, behind its own Access policy that names
# only its own people, and every other name gets a 404.

mock_provider "cloudflare" {
  mock_data "cloudflare_zone" {
    defaults = {
      zone_id = "0123456789abcdef0123456789abcdef"
    }
  }
}

variables {
  name          = "mandate-prod-cell1"
  account_id    = "fedcba9876543210fedcba9876543210"
  zone_name     = "owlhead.ai"
  workspace_ids = ["ws1", "ws2"]
  access_emails = {
    ws1 = ["founder@example.com"]
    ws2 = ["partner@example.com", "founder@example.com"]
  }
}

run "each_workspace_has_its_own_host_route_and_policy" {
  command = plan

  assert {
    condition = jsonencode([for rule in cloudflare_zero_trust_tunnel_cloudflared_config.this.config.ingress : [rule.hostname, rule.service]]) == jsonencode([
      ["api-ws1.owlhead.ai", "http://api.ws-ws1.svc.cluster.local:8080"],
      ["api-ws2.owlhead.ai", "http://api.ws-ws2.svc.cluster.local:8080"],
      [null, "http_status:404"],
    ])
    error_message = "Each host routes to its own workspace's API only, and the rest gets a 404."
  }

  assert {
    condition     = [for rule in nonsensitive(cloudflare_zero_trust_access_policy.workspace["ws1"].include) : rule.email.email] == ["founder@example.com"]
    error_message = "Workspace ws1's policy names only ws1's people."
  }

  assert {
    condition     = alltrue([for id, record in cloudflare_dns_record.api : record.proxied])
    error_message = "Every API record is proxied."
  }

  assert {
    condition     = output.access_users == 2
    error_message = "Two distinct people across the cell."
  }
}

run "refuses_a_workspace_with_no_named_person" {
  command = plan

  variables {
    access_emails = { ws1 = ["founder@example.com"] }
  }

  expect_failures = [cloudflare_zero_trust_access_policy.workspace]
}

run "refuses_a_wildcard_email" {
  command = plan

  variables {
    access_emails = { ws1 = ["*@example.com"], ws2 = ["partner@example.com"] }
  }

  expect_failures = [var.access_emails]
}

run "refuses_an_id_that_is_not_opaque" {
  command = plan

  variables {
    workspace_ids = ["Acme Corp"]
  }

  expect_failures = [var.workspace_ids]
}
