# One locked R2 bucket and its one writer (DEC-2064 item 4), planned against a mocked provider: the
# lock covers every key for the retention, and the token is scoped to this bucket alone.

mock_provider "cloudflare" {
  mock_data "cloudflare_account_api_token_permission_groups_list" {
    defaults = {
      result = [{ id = "c0ffee00c0ffee00c0ffee00c0ffee00", name = "Workers R2 Storage Bucket Item Write", category = "r2", is_selectable = true, scopes = ["com.cloudflare.edge.r2.bucket"] }]
    }
  }
}

variables {
  account_id     = "fedcba9876543210fedcba9876543210"
  bucket         = "mandate-vault-snapshots-prod-cell1"
  token_name     = "mandate-prod-cell1-vault-snapshots"
  retention_days = 30
}

run "the_lock_covers_every_key_and_the_token_one_bucket" {
  command = plan

  assert {
    condition     = length(cloudflare_r2_bucket_lock.this.rules) == 1 && cloudflare_r2_bucket_lock.this.rules[0].enabled && cloudflare_r2_bucket_lock.this.rules[0].prefix == "" && cloudflare_r2_bucket_lock.this.rules[0].condition.max_age_seconds == 2592000
    error_message = "One enabled rule over every key, for 30 days."
  }

  assert {
    condition     = keys(jsondecode(cloudflare_account_token.writer.policies[0].resources)) == ["com.cloudflare.edge.r2.bucket.fedcba9876543210fedcba9876543210_default_mandate-vault-snapshots-prod-cell1"]
    error_message = "The writer's token names this bucket and nothing else."
  }

  assert {
    condition     = cloudflare_r2_bucket.this.location == "wnam"
    error_message = "The bucket is in western North America."
  }
}

run "refuses_a_short_retention" {
  command = plan

  variables {
    retention_days = 7
  }

  expect_failures = [var.retention_days]
}
