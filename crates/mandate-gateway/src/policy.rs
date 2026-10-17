//! Which endpoints a workspace may use (E15-10, INF-12) and the egress allowlist that follows from
//! it (inference spec §8.2). Until the policy key is specified in `policy.schema.json` (inference
//! spec §13 question 2), the allowed localities, upstream providers, and regions come from
//! deployment configuration, and an absent policy allows nothing hosted: local-only (DEC-432
//! item 8).

use std::collections::BTreeSet;

use crate::registry::{Endpoint, Locality, Registry};

/// The endpoints a workspace may call. `None` for providers or regions means the level sets no
/// restriction on them; localities are always listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointPolicy {
    localities: BTreeSet<Locality>,
    providers: Option<BTreeSet<String>>,
    regions: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("a child policy allows something its parent does not (child ⊆ parent)")]
    Widens,
}

impl Default for EndpointPolicy {
    fn default() -> Self {
        Self::local_only()
    }
}

impl EndpointPolicy {
    /// The default for hybrid and on-prem, and for any workspace with no policy: no hosted
    /// endpoint at all (DEC-432 item 8).
    pub fn local_only() -> Self {
        Self {
            localities: BTreeSet::from([Locality::Local]),
            providers: None,
            regions: None,
        }
    }

    pub fn new(
        localities: BTreeSet<Locality>,
        providers: Option<BTreeSet<String>>,
        regions: Option<BTreeSet<String>>,
    ) -> Self {
        Self {
            localities,
            providers,
            regions,
        }
    }

    /// Whether a call may go to `endpoint`.
    pub fn allows(&self, endpoint: &Endpoint) -> bool {
        self.localities.contains(&endpoint.locality)
            && self
                .providers
                .as_ref()
                .is_none_or(|p| p.contains(&endpoint.route.upstream))
            && self
                .regions
                .as_ref()
                .is_none_or(|r| r.contains(&endpoint.region))
    }

    /// `child` checked against this policy as its parent (mandate spec §4.3, sets: child ⊆ parent),
    /// and the policy in force: the child's, which can only be narrower. A child that leaves
    /// providers or regions unset inherits the parent's.
    pub fn narrowed(&self, child: &Self) -> Result<Self, PolicyError> {
        let subset = |parent: &Option<BTreeSet<String>>, child: &Option<BTreeSet<String>>| match (
            parent, child,
        ) {
            (Some(p), Some(c)) => c.is_subset(p),
            (Some(_), None) | (None, _) => true,
        };
        if !child.localities.is_subset(&self.localities)
            || !subset(&self.providers, &child.providers)
            || !subset(&self.regions, &child.regions)
        {
            return Err(PolicyError::Widens);
        }
        Ok(Self {
            localities: child.localities.clone(),
            providers: child.providers.clone().or_else(|| self.providers.clone()),
            regions: child.regions.clone().or_else(|| self.regions.clone()),
        })
    }
}

/// The hosts the gateway may open a connection to: those of the registry's endpoints that the
/// policy allows, and no other (inference spec §8.2). The same list configures the network layer;
/// this is the in-code half.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EgressAllowlist(BTreeSet<String>);

impl EgressAllowlist {
    pub fn of(registry: &Registry, policy: &EndpointPolicy) -> Self {
        Self(
            registry
                .entries()
                .flat_map(|e| e.endpoints().map(|(endpoint, _)| endpoint))
                .filter(|endpoint| policy.allows(endpoint))
                .map(|endpoint| endpoint.host.clone())
                .collect(),
        )
    }

    pub fn permits(&self, host: &str) -> bool {
        self.0.contains(host)
    }

    pub fn hosts(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}
