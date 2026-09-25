//! Anchoring (journal spec §10): stream heads as leaves of an RFC 6962-style Merkle tree.

use mandate_canon::Digest;

/// One stream head covered by an anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorLeaf {
    pub stream_id: String,
    pub seq: u64,
    pub hash: Digest,
}

impl AnchorLeaf {
    /// SHA-256(0x00 ‖ canonical `{hash, seq, stream_id}`); `None` if `seq` exceeds 2^53 − 1.
    pub fn leaf_hash(&self) -> Option<Digest> {
        None
    }
}

/// The leaves, sorted by `stream_id` bytes, and their root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub leaves: Vec<AnchorLeaf>,
    pub root: Digest,
}

impl Anchor {
    /// Sorts the heads by `stream_id` and computes the root; `None` for no heads, a repeated
    /// stream, or an out-of-range `seq`.
    pub fn compute(_heads: Vec<AnchorLeaf>) -> Option<Self> {
        None
    }
}

/// The root over `leaves` in the given order: node = SHA-256(0x01 ‖ left ‖ right), splitting at
/// the largest power of two below n; a single leaf is its own root. `None` for no leaves.
pub fn merkle_root(_leaves: &[AnchorLeaf]) -> Option<Digest> {
    None
}

/// The RFC 3161 message imprint: SHA-256 of the 32 raw root bytes.
pub fn tsa_imprint(_root: &Digest) -> Digest {
    Digest::ZERO
}
