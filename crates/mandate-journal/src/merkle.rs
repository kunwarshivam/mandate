//! Anchoring (journal spec §10): stream heads as leaves of an RFC 6962-style Merkle tree.

use mandate_canon::{Digest, Int, Key, Object, Value, to_canonical};

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
        let mut object = Object::new();
        object.insert(Key::new("hash").ok()?, Value::Str(self.hash.to_hex()));
        object.insert(Key::new("seq").ok()?, Value::Int(Int::new(self.seq)?));
        object.insert(
            Key::new("stream_id").ok()?,
            Value::Str(self.stream_id.clone()),
        );
        Some(Digest::of_parts(&[
            &[0x00],
            &to_canonical(&Value::Object(object)),
        ]))
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
    pub fn compute(mut heads: Vec<AnchorLeaf>) -> Option<Self> {
        heads.sort_by(|a, b| a.stream_id.cmp(&b.stream_id));
        if heads
            .windows(2)
            .any(|w| matches!(w, [a, b] if a.stream_id == b.stream_id))
        {
            return None;
        }
        let root = merkle_root(&heads)?;
        Some(Self {
            leaves: heads,
            root,
        })
    }
}

/// The root over `leaves` in the given order: node = SHA-256(0x01 ‖ left ‖ right), splitting at
/// the largest power of two below n; a single leaf is its own root. `None` for no leaves.
pub fn merkle_root(leaves: &[AnchorLeaf]) -> Option<Digest> {
    let hashes = leaves
        .iter()
        .map(AnchorLeaf::leaf_hash)
        .collect::<Option<Vec<_>>>()?;
    root_of(&hashes)
}

/// With no leaves, `split_at_checked` fails and the root is `None`.
fn root_of(hashes: &[Digest]) -> Option<Digest> {
    match hashes {
        [single] => Some(*single),
        _ => {
            let mut split = 1usize;
            while split.checked_mul(2)? < hashes.len() {
                split = split.checked_mul(2)?;
            }
            let (left, right) = hashes.split_at_checked(split)?;
            let (left, right) = (root_of(left)?, root_of(right)?);
            Some(Digest::of_parts(&[
                &[0x01],
                left.as_bytes(),
                right.as_bytes(),
            ]))
        }
    }
}

/// The RFC 3161 message imprint: SHA-256 of the 32 raw root bytes.
pub fn tsa_imprint(root: &Digest) -> Digest {
    Digest::of(root.as_bytes())
}
