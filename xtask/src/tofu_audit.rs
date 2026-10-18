//! What keeps `infra.yml`'s public log free of values (E21-56, DEC-2030 item 6): the providers'
//! own `sensitive` flags on every attribute a credential could sit in, a plan summary that names
//! only addresses and actions, and GitHub's masks for every value fetched at run time. The
//! repository is public, so its Actions logs are too, and GitHub masks only the secrets it
//! handed the job, never a value a step fetched.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::image::base64;

/// The attribute names a credential is held under, in any resource or data source this tree uses.
/// Each must be `sensitive` in its provider's schema, so a plan's diff prints it as `(sensitive
/// value)`.
const CREDENTIAL_NAMES: [&str; 8] = [
    "token",
    "secret",
    "password",
    "private_key",
    "kube_config",
    "raw_config",
    "access_key",
    "value",
];

/// The reviewed attributes that carry a [`CREDENTIAL_NAMES`] name but hold no credential, each as
/// `type` and dotted path: a DNS record's data value, and a node taint's value.
const NOT_CREDENTIALS: [(&str, &str); 2] = [
    ("cloudflare_dns_record", "data.value"),
    ("digitalocean_kubernetes_cluster", "node_pool.taint.value"),
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

/// Every attribute named in [`CREDENTIAL_NAMES`] that `schema` (`tofu providers schema -json`)
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
        if CREDENTIAL_NAMES.contains(&name.as_str()) && !sensitive {
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

/// The forms of `value` a log could carry, each of which GitHub must mask: the value itself, its
/// standard and URL-safe base64 with and without padding, its URL encoding, its JSON-escaped
/// form, and each of its lines. Empty forms, and a multi-line value's whole form, which one
/// workflow command cannot carry, are dropped; its lines are masked one by one.
pub(crate) fn mask_forms(value: &str) -> BTreeSet<String> {
    let standard = base64(value.as_bytes());
    let url_safe = standard.replace('+', "-").replace('/', "_");
    let json = serde_json::to_string(value).unwrap_or_default();
    let mut forms: BTreeSet<String> = [
        value.to_owned(),
        standard.trim_end_matches('=').to_owned(),
        url_safe.trim_end_matches('=').to_owned(),
        standard,
        url_safe,
        percent_encoded(value),
        json.get(1..json.len().saturating_sub(1))
            .unwrap_or_default()
            .to_owned(),
    ]
    .into();
    forms.extend(value.lines().map(str::to_owned));
    forms.retain(|form| !form.trim().is_empty() && !form.contains(['\n', '\r']));
    forms
}

/// `value` percent-encoded, every byte but RFC 3986's unreserved ones.
fn percent_encoded(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// The workflow commands that mask `value` in every form (see [`mask_forms`]); GitHub hides the
/// commands themselves from the log.
pub(crate) fn mask_commands(value: &str) -> Vec<String> {
    mask_forms(value)
        .into_iter()
        .map(|form| format!("::add-mask::{form}"))
        .collect()
}

/// Masks `value` when running in GitHub Actions, before anything else can print it.
pub(crate) fn mask(value: &str) {
    if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        for command in mask_commands(value) {
            println!("{command}");
        }
    }
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

    /// The audit's item 3: every form a log could carry is masked.
    #[test]
    fn every_form_of_a_value_is_masked() {
        let value = "ab+/c\"d\nsecond line";
        let forms = mask_forms(value);
        let standard = base64(value.as_bytes());
        assert!(
            !forms.contains(value),
            "a multi-line form would break its command"
        );
        assert!(mask_forms("single").contains("single"));
        for form in [
            standard.clone(),
            standard.replace('+', "-").replace('/', "_"),
            standard.trim_end_matches('=').to_owned(),
            "ab%2B%2Fc%22d%0Asecond%20line".to_owned(),
            "ab+/c\\\"d\\nsecond line".to_owned(),
            "ab+/c\"d".to_owned(),
            "second line".to_owned(),
        ] {
            assert!(forms.contains(&form), "{form:?} is not masked: {forms:?}");
        }
        assert!(mask_forms("").is_empty(), "an empty value masks nothing");
        assert!(
            mask_commands("x")
                .iter()
                .all(|command| command.starts_with("::add-mask::"))
        );
    }
}
