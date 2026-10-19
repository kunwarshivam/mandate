# The API host is reached only through the tunnel and only past Access (DEC-2012 items 6 and 8),
# checked on the plan against a mocked provider.

mock_provider "cloudflare" {
  mock_data "cloudflare_account_api_token_permission_groups_list" {
    defaults = {
      result = [{ id = "c0ffee00c0ffee00c0ffee00c0ffee00", name = "Workers R2 Storage Bucket Item Write", category = "r2", is_selectable = true, scopes = ["com.cloudflare.edge.r2.bucket"] }]
    }
  }

  mock_data "cloudflare_zone" {
    defaults = {
      zone_id = "0123456789abcdef0123456789abcdef"
    }
  }

  mock_resource "cloudflare_zero_trust_tunnel_cloudflared" {
    defaults = {
      id = "c0ffee00-0000-4000-8000-000000000000"
    }
  }
}

variables {
  name           = "mandate-staging"
  account_id     = "fedcba9876543210fedcba9876543210"
  zone_name      = "example.com"
  api_hostname   = "api-staging.example.com"
  api_origin     = "http://127.0.0.1:8080"
  archive_bucket = "mandate-journal-archive-west-staging"
}

run "api_host_is_tunnel_only_and_behind_access" {
  command = plan

  assert {
    condition     = cloudflare_dns_record.api.proxied && cloudflare_dns_record.api.type == "CNAME" && cloudflare_dns_record.api.content == "c0ffee00-0000-4000-8000-000000000000.cfargotunnel.com"
    error_message = "The API host must be a proxied CNAME to the tunnel."
  }

  assert {
    condition     = cloudflare_zero_trust_tunnel_cloudflared.this.config_src == "cloudflare"
    error_message = "The tunnel is remotely managed, so its token is all the chart needs."
  }

  assert {
    condition = (
      length(cloudflare_zero_trust_tunnel_cloudflared_config.this.config.ingress) == 2 &&
      cloudflare_zero_trust_tunnel_cloudflared_config.this.config.ingress[0].hostname == "api-staging.example.com" &&
      cloudflare_zero_trust_tunnel_cloudflared_config.this.config.ingress[1].service == "http_status:404"
    )
    error_message = "The tunnel routes the API host and answers 404 to everything else."
  }

  assert {
    condition     = cloudflare_zero_trust_access_application.api.domain == "api-staging.example.com" && cloudflare_zero_trust_access_application.api.type == "self_hosted"
    error_message = "Access must guard the API host."
  }

  assert {
    condition     = length(cloudflare_zero_trust_access_application.api.policies) == 1 && cloudflare_zero_trust_access_application.api.policies[0].id == cloudflare_zero_trust_access_policy.operators.id
    error_message = "The Access application must carry the operators' policy and no other."
  }

  assert {
    condition = (
      cloudflare_zero_trust_access_policy.operators.decision == "allow" &&
      length(cloudflare_zero_trust_access_policy.operators.include) == 1 &&
      alltrue([for rule in cloudflare_zero_trust_access_policy.operators.include : rule.cloudflare_account_member.account_id == "fedcba9876543210fedcba9876543210" && rule.everyone == null && rule.email_domain == null])
    )
    error_message = "The policy admits members of the founder's Cloudflare account alone, never everyone or a domain."
  }
}

run "the_archive_is_locked_for_its_retention" {
  command = plan

  assert {
    condition     = cloudflare_r2_bucket.archive.name == "mandate-journal-archive-west-staging" && cloudflare_r2_bucket.archive.location == "wnam" && cloudflare_r2_bucket.archive.jurisdiction == "default"
    error_message = "The archive is the environment's R2 bucket in western North America, in R2's default jurisdiction, whose endpoint the cell's egress is pinned to."
  }

  assert {
    condition = (
      length(cloudflare_r2_bucket_lock.archive.rules) == 1 &&
      cloudflare_r2_bucket_lock.archive.rules[0].enabled &&
      cloudflare_r2_bucket_lock.archive.rules[0].prefix == "" &&
      cloudflare_r2_bucket_lock.archive.rules[0].condition.type == "Age" &&
      cloudflare_r2_bucket_lock.archive.rules[0].condition.max_age_seconds >= 30 * 86400
    )
    error_message = "One enabled lock rule must cover every key for at least the 30-day retention."
  }

  assert {
    condition     = cloudflare_r2_bucket_lock.archive.bucket_name == cloudflare_r2_bucket.archive.name
    error_message = "The lock is on the archive bucket."
  }
}

run "barman_token_reaches_the_archive_bucket_alone" {
  command = plan

  assert {
    condition = (
      length(cloudflare_account_token.journal_archive.policies) == 1 &&
      cloudflare_account_token.journal_archive.policies[0].effect == "allow" &&
      length(cloudflare_account_token.journal_archive.policies[0].permission_groups) == 1 &&
      cloudflare_account_token.journal_archive.policies[0].permission_groups[0].id == "c0ffee00c0ffee00c0ffee00c0ffee00" &&
      jsondecode(cloudflare_account_token.journal_archive.policies[0].resources) == { "com.cloudflare.edge.r2.bucket.fedcba9876543210fedcba9876543210_default_mandate-journal-archive-west-staging" = "*" }
    )
    error_message = "The minted token holds Object Read and Write on the archive bucket and nothing else."
  }
}

run "refuses_an_ambiguous_permission_group" {
  command = plan

  override_data {
    target = data.cloudflare_account_api_token_permission_groups_list.r2_object_write
    values = {
      result = []
    }
  }

  expect_failures = [cloudflare_account_token.journal_archive]
}

run "refuses_a_lock_shorter_than_the_retention" {
  command = plan

  variables {
    archive_retention_days = 7
  }

  expect_failures = [var.archive_retention_days]
}

run "named_emails_replace_account_membership" {
  command = plan

  variables {
    access_allowed_emails = ["founder@example.com"]
  }

  assert {
    condition = (
      length(cloudflare_zero_trust_access_policy.operators.include) == 1 &&
      alltrue([for rule in cloudflare_zero_trust_access_policy.operators.include : rule.email.email == "founder@example.com" && rule.cloudflare_account_member == null && rule.everyone == null])
    )
    error_message = "With emails named, the policy admits those emails alone, not the account's members."
  }
}

run "refuses_a_wildcard_email" {
  command = plan

  variables {
    access_allowed_emails = ["*@example.com"]
  }

  expect_failures = [var.access_allowed_emails]
}

run "refuses_a_spaced_email" {
  command = plan

  variables {
    access_allowed_emails = ["founder @example.com"]
  }

  expect_failures = [var.access_allowed_emails]
}

# DEC-2047: the archive stays in western North America; any other location hint is refused.
run "refuses_an_archive_outside_the_west" {
  command = plan

  variables {
    archive_location = "enam"
  }

  expect_failures = [var.archive_location]
}
