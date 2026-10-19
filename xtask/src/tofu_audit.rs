//! What keeps `infra.yml`'s public log free of values (E21-56, DEC-2014 item 4): the repository
//! is public, so its Actions logs are too, and a plan's diff or an apply's `Outputs:` block would
//! print the founder's addresses and the environments' ids. The log gets a plan summary that names
//! only addresses and actions.

use serde_json::Value;

/// A saved plan (`tofu show -json <plan>`) as the log may show it: one line per resource change,
/// its actions and address, and nothing of its values. A plan that names no change says so.
pub(crate) fn plan_summary(plan: &Value) -> Vec<String> {
    let changes: Vec<String> = plan
        .get("resource_changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|change| {
            let address = change.get("address")?.as_str()?;
            let actions: Vec<&str> = change
                .get("change")?
                .get("actions")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .collect();
            (actions != ["no-op"] && actions != ["read"])
                .then(|| format!("{} {address}", actions.join("+")))
        })
        .collect();
    if changes.is_empty() {
        vec!["no resource changes".to_owned()]
    } else {
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The audit's item 1: the plan summary carries addresses and actions and no value.
    #[test]
    fn the_plan_summary_holds_no_value() {
        let plan = serde_json::json!({
            "resource_changes": [
                { "address": "module.cell.module.doks.digitalocean_kubernetes_cluster.this",
                  "change": { "actions": ["create"],
                              "after": { "control_plane_firewall": [{ "allowed_addresses": ["203.0.113.7/32"] }] } } },
                { "address": "module.cloudflare.cloudflare_dns_record.api",
                  "change": { "actions": ["delete", "create"], "after": { "content": "c0ffee.cfargotunnel.com" } } },
                { "address": "data.cloudflare_accounts.visible",
                  "change": { "actions": ["read"] } },
                { "address": "terraform_data.applied_commit",
                  "change": { "actions": ["no-op"] } }
            ],
            "variables": { "kube_api_allowed_addresses": { "value": ["203.0.113.7/32"] } }
        });
        let summary = plan_summary(&plan);
        assert_eq!(
            summary,
            [
                "create module.cell.module.doks.digitalocean_kubernetes_cluster.this",
                "delete+create module.cloudflare.cloudflare_dns_record.api",
            ]
        );
        for value in ["203.0.113.7", "cfargotunnel"] {
            assert!(!summary.join("\n").contains(value), "{value} printed");
        }
        assert_eq!(
            plan_summary(&serde_json::json!({})),
            ["no resource changes"]
        );
    }
}
