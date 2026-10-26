# The cell's out-of-cell alerts (DEC-2064 item 6), planned against a mocked provider: node alerts on
# the cell's tag and one uptime check with its down alert per API host.

mock_provider "digitalocean" {}

variables {
  name         = "mandate-prod-cell1"
  node_tag     = "mandate-prod-cell1-nodes"
  api_hosts    = { ws1 = "api-ws1.owlhead.ai", ws2 = "api-ws2.owlhead.ai" }
  alert_emails = ["founder@example.com"]
}

run "alerts_cover_every_node_and_host" {
  command = plan

  assert {
    condition     = length(digitalocean_monitor_alert.node) == 3 && alltrue([for alert in digitalocean_monitor_alert.node : alert.tags == toset(["mandate-prod-cell1-nodes"])])
    error_message = "CPU, memory and disk alerts on the cell's node tag."
  }

  assert {
    condition     = output.uptime_checks == 2 && digitalocean_uptime_check.api["ws1"].target == "https://api-ws1.owlhead.ai/"
    error_message = "One check per API host."
  }
}

run "refuses_no_recipient" {
  command = plan

  variables {
    alert_emails = []
  }

  expect_failures = [var.alert_emails]
}
