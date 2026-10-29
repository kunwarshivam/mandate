//! What keeps `infra.yml`'s public log free of values (E21-56, DEC-2030 item 6): the providers'
//! own `sensitive` flags on every attribute a credential could sit in, a plan summary that names
//! only addresses and actions, and which items of a list secret to mask. The masks themselves are
//! [`crate::mask`]'s, the one place a workflow command `::add-mask::` is written. The
//! repository is public, so its Actions logs are too, and GitHub masks only the secrets it
//! handed the job, never a value a step fetched.

use std::collections::BTreeSet;

use serde_json::Value;

/// The name parts a credential is held under, in any resource or data source this tree uses: an
/// attribute whose name contains one (`tunnel_secret`, `client_secret`, `api_key`,
/// `secret_access_key`), or is exactly `value`, and that holds a string, must be `sensitive` in
/// its provider's schema, so a plan's diff prints it as `(sensitive value)`. An attribute of
/// another type (an Access rule's `service_token` object, a boolean such as
/// `allow_pkce_without_client_secret`) holds no credential and is not held to it.
const CREDENTIAL_PARTS: [&str; 8] = [
    "token",
    "secret",
    "password",
    "private_key",
    "kube_config",
    "raw_config",
    "access_key",
    "api_key",
];

/// Whether an attribute named `name`, of the schema type `kind`, could hold a credential: a
/// string, or a list, set or map of strings, whose name carries a [`CREDENTIAL_PARTS`] part or is
/// `value`.
fn credential_named(name: &str, kind: Option<&Value>) -> bool {
    let stringly = match kind {
        Some(Value::String(kind)) => kind == "string",
        Some(Value::Array(parts)) => parts.last().and_then(Value::as_str) == Some("string"),
        _ => false,
    };
    let names_about_a_credential = NOT_CREDENTIAL_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix));
    stringly
        && !names_about_a_credential
        && (name == "value" || CREDENTIAL_PARTS.iter().any(|part| name.contains(part)))
}

/// The name endings that say an attribute is about a credential without holding one: an id
/// (`token_id`), a URL (`token_url`), a lifetime (`access_token_lifetime`) or a header's name
/// (`read_service_tokens_from_header`).
const NOT_CREDENTIAL_SUFFIXES: [&str; 4] = ["_id", "_url", "_lifetime", "_header"];

/// The reviewed attributes that carry a [`CREDENTIAL_PARTS`] name but hold no credential, each as
/// `type` and dotted path: a DNS record's data value, and a node taint's value, in the cluster's
/// own pool and in a separate pool (DEC-2064 item 3).
const NOT_CREDENTIALS: [(&str, &str); 3] = [
    ("cloudflare_dns_record", "data.value"),
    ("digitalocean_kubernetes_cluster", "node_pool.taint.value"),
    ("digitalocean_kubernetes_node_pool", "taint.value"),
];

/// Every `resource` and `data` type that `files` (each a path and its raw text) declare, as
/// `("resource" | "data", type)`, read from lines that open with the keyword.
pub(crate) fn used_types(files: &[(String, String)]) -> BTreeSet<(String, String)> {
    files
        .iter()
        .flat_map(|(_, text)| text.lines())
        .filter_map(|line| {
            let (kind, rest) = line.split_once(" \"")?;
            matches!(kind, "resource" | "data")
                .then(|| rest.split_once('"'))
                .flatten()
                .map(|(name, _)| (kind.to_owned(), name.to_owned()))
        })
        .collect()
}

/// Every attribute [`credential_named`] that `schema` (`tofu providers schema -json`)
/// leaves unmarked in a type `used` names, outside [`NOT_CREDENTIALS`], as `kind type: path`. A
/// type `used` names that no provider in `schema` describes is reported too, so a missing schema
/// never passes.
pub(crate) fn unmasked_attributes(
    schema: &Value,
    used: &BTreeSet<(String, String)>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let providers = schema
        .get("provider_schemas")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|providers| providers.values());
    let mut described = BTreeSet::new();
    for provider in providers {
        for (kind, key) in [
            ("resource", "resource_schemas"),
            ("data", "data_source_schemas"),
        ] {
            let Some(types) = provider.get(key).and_then(Value::as_object) else {
                continue;
            };
            for (name, entry) in types {
                if !used.contains(&(kind.to_owned(), name.clone())) {
                    continue;
                }
                described.insert((kind.to_owned(), name.clone()));
                let mut found = Vec::new();
                walk_block(entry.get("block"), &mut Vec::new(), &mut found);
                for path in found {
                    if !NOT_CREDENTIALS.contains(&(name.as_str(), path.as_str())) {
                        problems.push(format!(
                            "{kind} {name}: `{path}` is not sensitive in its provider's schema, so a \
                             plan would print it"
                        ));
                    }
                }
            }
        }
    }
    for (kind, name) in used {
        if kind == "resource" && name == "terraform_data" {
            continue;
        }
        if !described.contains(&(kind.clone(), name.clone())) {
            problems.push(format!("{kind} {name}: no provider schema describes it"));
        }
    }
    problems
}

/// Collects into `found` the dotted path of every unmarked credential-named attribute under
/// `block`, through nested attributes and nested blocks.
fn walk_block(block: Option<&Value>, path: &mut Vec<String>, found: &mut Vec<String>) {
    let Some(block) = block else {
        return;
    };
    walk_attributes(block.get("attributes"), path, found);
    if let Some(blocks) = block.get("block_types").and_then(Value::as_object) {
        for (name, nested) in blocks {
            path.push(name.clone());
            walk_block(nested.get("block"), path, found);
            path.pop();
        }
    }
}

fn walk_attributes(attributes: Option<&Value>, path: &mut Vec<String>, found: &mut Vec<String>) {
    let Some(attributes) = attributes.and_then(Value::as_object) else {
        return;
    };
    for (name, attribute) in attributes {
        path.push(name.clone());
        let sensitive = attribute.get("sensitive").and_then(Value::as_bool) == Some(true);
        if credential_named(name, attribute.get("type")) && !sensitive {
            found.push(path.join("."));
        }
        walk_attributes(
            attribute
                .get("nested_type")
                .and_then(|nested| nested.get("attributes")),
            path,
            found,
        );
        path.pop();
    }
}

/// The top-level attributes `change` gives a different value, or a value known only after the
/// apply, by name alone and sorted: the keys of its `before` and `after` whose values differ, a
/// key on one side alone included, and those `after_unknown` marks `true`.
fn changed_attributes(change: &Value) -> Vec<String> {
    let side = |name: &str| change.get(name).and_then(Value::as_object);
    let (before, after) = (side("before"), side("after"));
    let mut names: BTreeSet<String> = before
        .into_iter()
        .chain(after)
        .flat_map(|object| object.keys())
        .filter(|key| {
            before.and_then(|object| object.get(*key)) != after.and_then(|object| object.get(*key))
        })
        .cloned()
        .collect();
    names.extend(
        side("after_unknown")
            .into_iter()
            .flatten()
            .filter(|(_, unknown)| **unknown == Value::Bool(true))
            .map(|(key, _)| key.clone()),
    );
    names.into_iter().collect()
}

/// A saved plan (`tofu show -json <plan>`) as the log may show it: one line per resource change,
/// its actions and address, and nothing of its values. An update or a replacement also names the
/// attributes it changes ([`changed_attributes`]), so a diff that returns on every plan shows which
/// field moves; a create or a delete names none. A plan that names no change says so.
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
            if actions == ["no-op"] || actions == ["read"] {
                return None;
            }
            let line = format!("{} {address}", actions.join("+"));
            if actions == ["create"] || actions == ["delete"] {
                return Some(line);
            }
            let names = changed_attributes(change.get("change")?);
            Some(if names.is_empty() {
                format!("{line} (changes: none named)")
            } else {
                format!("{line} (changes: {})", names.join(", "))
            })
        })
        .collect();
    if changes.is_empty() {
        vec!["no resource changes".to_owned()]
    } else {
        changes
    }
}

/// What to mask of a JSON list secret, `KUBE_API_ALLOWED_CIDRS` or `ACCESS_ALLOWED_EMAILS`:
/// GitHub masks the secret only as the whole string it stored, so each item, and each CIDR's
/// bare address, is masked on its own before any `tofu` call could print one (the review's B2).
/// Text that is not a JSON list yields nothing, since GitHub already masks it whole.
pub(crate) fn list_items(json: &str) -> Vec<String> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items.iter().filter_map(Value::as_str) {
        out.push(item.to_owned());
        if let Some((host, _)) = item.split_once('/') {
            out.push(host.to_owned());
        }
    }
    out.retain(|item| !item.trim().is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
        pairs
            .iter()
            .map(|(kind, name)| ((*kind).to_owned(), (*name).to_owned()))
            .collect()
    }

    /// The audit's item 2: an unmarked credential attribute in a used type is named, through
    /// nested attributes and blocks; a marked one, an unused type and a reviewed exception pass.
    #[test]
    fn an_unmarked_credential_attribute_is_named() {
        let schema = serde_json::json!({
            "provider_schemas": {
                "registry.opentofu.org/cloudflare/cloudflare": {
                    "resource_schemas": {
                        "cloudflare_account_token": { "block": { "attributes": {
                            "value": { "type": "string", "sensitive": true },
                            "name": { "type": "string" }
                        } } },
                        "cloudflare_dns_record": { "block": { "attributes": {
                            "data": { "nested_type": { "attributes": {
                                "value": { "type": "string" }
                            } } }
                        } } },
                        "cloudflare_unused": { "block": { "attributes": {
                            "token": { "type": "string" }
                        } } }
                    },
                    "data_source_schemas": {
                        "cloudflare_zero_trust_tunnel_cloudflared_token": { "block": {
                            "attributes": { "token": { "type": "string" } },
                            "block_types": { "inner": { "block": { "attributes": {
                                "secret": { "type": "string" }
                            } } } }
                        } }
                    }
                }
            }
        });
        let types = used(&[
            ("resource", "cloudflare_account_token"),
            ("resource", "cloudflare_dns_record"),
            ("data", "cloudflare_zero_trust_tunnel_cloudflared_token"),
            ("resource", "terraform_data"),
        ]);
        let problems = unmasked_attributes(&schema, &types);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("`token`")));
        assert!(problems.iter().any(|p| p.contains("`inner.secret`")));
        assert!(credential_named(
            "tunnel_secret",
            Some(&serde_json::json!("string"))
        ));
        assert!(credential_named(
            "client_secret",
            Some(&serde_json::json!("string"))
        ));
        assert!(credential_named(
            "secret_access_key",
            Some(&serde_json::json!("string"))
        ));
        assert!(credential_named(
            "api_key",
            Some(&serde_json::json!(["list", "string"]))
        ));
        assert!(!credential_named(
            "service_token",
            Some(&serde_json::json!(["object", {}]))
        ));
        assert!(!credential_named(
            "allow_pkce_without_client_secret",
            Some(&serde_json::json!("bool"))
        ));
        assert!(!credential_named(
            "name",
            Some(&serde_json::json!("string"))
        ));
        let missing = unmasked_attributes(&schema, &used(&[("resource", "cloudflare_other")]));
        assert_eq!(
            missing,
            ["resource cloudflare_other: no provider schema describes it"]
        );
    }

    /// Types are read from the lines that declare them, and nothing else.
    #[test]
    fn used_types_reads_declarations() {
        let files = vec![(
            "main.tf".to_owned(),
            "resource \"digitalocean_vpc\" \"this\" {\n  name = \"resource \\\"x\\\"\"\n}\n\
             data \"cloudflare_zone\" \"this\" {}\n  data \"indented\" \"x\" {}\n"
                .to_owned(),
        )];
        assert_eq!(
            used_types(&files),
            used(&[
                ("resource", "digitalocean_vpc"),
                ("data", "cloudflare_zone")
            ])
        );
    }

    /// An update or a replacement names the attributes it changes, by name alone, so a diff that
    /// returns on every plan (staging's cluster `update` of 2026-10-29) shows which field moves; a
    /// create or a delete, which changes every attribute, names none.
    #[test]
    fn an_update_names_the_attributes_it_changes_and_no_value() {
        let plan = serde_json::json!({
            "resource_changes": [
                { "address": "module.cell.module.doks.digitalocean_kubernetes_cluster.this",
                  "change": { "actions": ["update"],
                              "before": { "version": "1.35.9-do.1", "name": "mandate-staging",
                                          "tags": ["a"], "dropped": "gone" },
                              "after": { "version": "1.35.9-do.2", "name": "mandate-staging",
                                         "tags": ["a"] },
                              "after_unknown": { "updated_at": true, "name": false } } },
                { "address": "module.cloudflare.cloudflare_dns_record.api",
                  "change": { "actions": ["delete", "create"],
                              "before": { "content": "old.cfargotunnel.com" },
                              "after": { "content": "new.cfargotunnel.com" } } },
                { "address": "module.cloudflare.cloudflare_zero_trust_tunnel_cloudflared.this",
                  "change": { "actions": ["create"], "after": { "name": "secret-name" } } },
                { "address": "terraform_data.applied_commit",
                  "change": { "actions": ["update"], "before": { "input": "x" },
                              "after": { "input": "x" } } }
            ]
        });
        let summary = plan_summary(&plan);
        assert_eq!(
            summary,
            [
                "update module.cell.module.doks.digitalocean_kubernetes_cluster.this (changes: dropped, updated_at, version)",
                "delete+create module.cloudflare.cloudflare_dns_record.api (changes: content)",
                "create module.cloudflare.cloudflare_zero_trust_tunnel_cloudflared.this",
                "update terraform_data.applied_commit (changes: none named)",
            ]
        );
        for value in ["1.35.9", "mandate-staging", "cfargotunnel", "secret-name"] {
            assert!(!summary.join("\n").contains(value), "{value} printed");
        }
    }

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
                "delete+create module.cloudflare.cloudflare_dns_record.api (changes: content)",
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

    /// The review's B2: each item of a list secret, and each CIDR's bare address, is masked on its
    /// own.
    #[test]
    fn each_item_of_a_list_secret_is_masked() {
        assert_eq!(
            list_items("[\"203.0.113.7/32\", \"2001:db8::/48\"]"),
            [
                "203.0.113.7/32",
                "203.0.113.7",
                "2001:db8::/48",
                "2001:db8::"
            ]
        );
        assert_eq!(list_items("[\"you@example.com\"]"), ["you@example.com"]);
        assert!(list_items("").is_empty());
        assert!(list_items("not json").is_empty());
        assert!(list_items("[\"\", 7]").is_empty());
    }

    /// DEC-2054: no plan can destroy a VPC, since DigitalOcean refuses to delete a region's default
    /// VPC and an apply that tried failed halfway: every VPC the tree declares keeps
    /// `prevent_destroy`.
    #[test]
    fn every_vpc_is_protected_from_destruction() {
        let network = include_str!("../../deploy/tofu/modules/network/main.tf");
        let blocks: Vec<&str> = network
            .split("resource \"digitalocean_vpc\" ")
            .skip(1)
            .map(|rest| rest.split("\n}\n").next().unwrap_or_default())
            .collect();
        assert_eq!(blocks.len(), 1, "the network module declares one VPC");
        for block in blocks {
            assert!(
                block.contains("prevent_destroy = true"),
                "a VPC without prevent_destroy: {block}"
            );
        }
    }
}
