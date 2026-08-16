//! MiMC Tornado tree primitives (SPEC-onchain-mimc-tornado). Ports
//! `sealed_app/lib/infra/zk/mimc_tree.dart` (leaf/nullifier/zero-ladder)
//! and `mimc_path.dart` (membership-path construction) — folded into one
//! file since both are small and tightly coupled to the same tree shape.
//!
//! Not wired into any Tauri command or the redeem flow yet — see
//! [`crate::zk::mimc`]'s module doc comment.

use std::sync::LazyLock;

use num_bigint::BigUint;

use super::mimc::{mimc1, mimc2, MimcError};

/// `NULL_TAG = bytes("SEALED_NULL_V1__")` as Fr (`redeem.circom` line 95).
/// Domain separation so the nullifier can't collide with a leaf.
pub static MIMC_NULL_TAG: LazyLock<BigUint> =
    LazyLock::new(|| "110815058874449983093867477470989876063".parse().expect("MIMC_NULL_TAG is a valid decimal literal"));

/// On-chain incremental tree height.
pub const MIMC_TREE_HEIGHT: usize = 16;

/// leaf = MiMCHash(preimage). Submitted on-chain as the deposit leaf.
pub fn mimc_leaf_from_preimage(preimage: &BigUint) -> Result<BigUint, MimcError> {
    mimc1(preimage)
}

/// nullifier = MiMCHash(preimage, NULL_TAG). Public signal; double-spend guard.
pub fn mimc_nullifier_from_preimage(preimage: &BigUint) -> Result<BigUint, MimcError> {
    mimc2(preimage, &MIMC_NULL_TAG)
}

/// Zero ladder: `zeros[0] = 0`, `zeros[k+1] = mimc(zeros[k] || zeros[k])`.
/// `zeros[height]` is the empty-tree root. Computed once.
pub static MIMC_ZEROS: LazyLock<Vec<BigUint>> = LazyLock::new(|| {
    let mut z = vec![BigUint::from(0u32)];
    for i in 0..MIMC_TREE_HEIGHT {
        let next = mimc2(&z[i], &z[i]).expect("mimc2 of a canonical zero-ladder element with itself cannot fail");
        z.push(next);
    }
    z
});

/// A reconstructed Merkle membership path.
pub struct MimcMerklePath {
    /// Sibling at each of the [`MIMC_TREE_HEIGHT`] levels.
    pub path_elements: Vec<BigUint>,
    /// Per level: `0` = current node is the left child, `1` = right child.
    pub path_indices: Vec<u8>,
    /// Root produced by walking the path (the tree's current root).
    pub root: BigUint,
}

/// Index of `leaf` in the ordered list, or `None` if absent (not yet indexed).
pub fn mimc_find_leaf_index(leaves: &[BigUint], leaf: &BigUint) -> Option<usize> {
    leaves.iter().position(|l| l == leaf)
}

/// Build the membership path for the leaf at `leaf_index` from the full
/// ordered leaf list. Pads each level with the level's zero, hashing with
/// `mimc2` exactly as the contract's incremental tree does. The returned
/// root is the CURRENT tree root (after every deposit in `leaves`) — the
/// value to check against the on-chain ring and prove against.
pub fn mimc_path_from_leaves(leaves: &[BigUint], leaf_index: usize) -> Result<MimcMerklePath, MimcError> {
    if leaf_index >= leaves.len() {
        return Err(MimcError::LeafIndexOutOfRange { index: leaf_index, len: leaves.len() });
    }
    let mut path_elements = Vec::with_capacity(MIMC_TREE_HEIGHT);
    let mut path_indices = Vec::with_capacity(MIMC_TREE_HEIGHT);

    let mut level: Vec<BigUint> = leaves.to_vec();
    let mut idx = leaf_index;

    for h in 0..MIMC_TREE_HEIGHT {
        let is_right = (idx & 1) as u8;
        let sibling_idx = if is_right == 1 { idx - 1 } else { idx + 1 };
        let sibling = if sibling_idx < level.len() { level[sibling_idx].clone() } else { MIMC_ZEROS[h].clone() };
        path_elements.push(sibling);
        path_indices.push(is_right);

        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i < level.len() {
            let l = &level[i];
            let r = if i + 1 < level.len() { &level[i + 1] } else { &MIMC_ZEROS[h] };
            next.push(mimc2(l, r)?);
            i += 2;
        }
        level = if !next.is_empty() { next } else { vec![MIMC_ZEROS[h + 1].clone()] };
        idx >>= 1;
    }

    Ok(MimcMerklePath { path_elements, path_indices, root: level[0].clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(s: &str) -> BigUint {
        s.parse().unwrap()
    }

    /// Leftmost-leaf (index 0) root: every sibling is the level's zero.
    /// Mirrors `mimc_bn254_test.dart`'s `_leftmostRoot`.
    fn leftmost_root(leaf: &BigUint) -> BigUint {
        let mut cur = leaf.clone();
        for level in 0..MIMC_TREE_HEIGHT {
            cur = mimc2(&cur, &MIMC_ZEROS[level]).unwrap();
        }
        cur
    }

    /// Independent reference: walk a leaf up using the supplied path +
    /// indices. Mirrors `mimc_path_test.dart`'s `_rootFromPath`.
    fn root_from_path(leaf: &BigUint, p: &MimcMerklePath) -> BigUint {
        let mut cur = leaf.clone();
        for i in 0..MIMC_TREE_HEIGHT {
            cur = if p.path_indices[i] == 1 { mimc2(&p.path_elements[i], &cur).unwrap() } else { mimc2(&cur, &p.path_elements[i]).unwrap() };
        }
        cur
    }

    /// Golden vectors from `programs/sealed/test/snark-vectors/` — same
    /// three (`vector-01-basic`, `vector-02-deep`, `vector-07-edge0`)
    /// `mimc_bn254_test.dart` checks. All three are `leafIndex: 0`
    /// single-leaf-tree vectors (verified against the committed JSON), so
    /// the vector's `root` is exactly the leftmost-path root and their
    /// `pathElements` are exactly the zero ladder.
    struct GoldenVector {
        preimage: &'static str,
        leaf: &'static str,
        nullifier: &'static str,
        root: &'static str,
    }

    const GOLDEN_VECTORS: [GoldenVector; 3] = [
        GoldenVector {
            preimage: "15244084245341884415681856518087454146348696453121642827805482506755218510697",
            leaf: "13424948516919360545634583438959378442368048150777182565169285816652245550424",
            nullifier: "6011765299199332955189723703777223383819081556329267433704432112677725259883",
            root: "2483068863593819521746911350982727386752578097166174143696062544068720586344",
        },
        GoldenVector {
            preimage: "4439811776931355653183881931034480321574487875890496039201844210810244767442",
            leaf: "105777415219090828616850881925799653786314300651897545496363811011133294611",
            nullifier: "8342719796442355962706435081989888498971303967537453755782392277762171365895",
            root: "19029299205657441607397879628542255359617659371469874725095895128478351875403",
        },
        GoldenVector {
            preimage: "20161988821146474139943049994698937974290428847462166220863896737394176398805",
            leaf: "12998959745535602171322427477779490975891729445795761625146977218073424724289",
            nullifier: "18340915829791778334440379480604433437532232161318953618588998895114818649703",
            root: "11577570125397798789901607653722506431577669406081705242048761370296744625822",
        },
    ];

    #[test]
    fn mimc_parity_golden_vectors() {
        for v in &GOLDEN_VECTORS {
            let preimage = big(v.preimage);
            let expected_leaf = big(v.leaf);
            let expected_nullifier = big(v.nullifier);
            let expected_root = big(v.root);

            assert_eq!(mimc_leaf_from_preimage(&preimage).unwrap(), expected_leaf, "leaf mismatch");
            assert_eq!(mimc_nullifier_from_preimage(&preimage).unwrap(), expected_nullifier, "nullifier mismatch");
            assert_eq!(leftmost_root(&expected_leaf), expected_root, "root mismatch");
        }
    }

    #[test]
    fn empty_tree_root_matches_zeros_height() {
        assert_eq!(MIMC_ZEROS.len(), MIMC_TREE_HEIGHT + 1);
    }

    #[test]
    fn single_leaf_index_0_matches_golden_vector_and_zero_siblings() {
        let leaf = big(GOLDEN_VECTORS[0].leaf);
        let expected_root = big(GOLDEN_VECTORS[0].root);

        let p = mimc_path_from_leaves(&[leaf.clone()], 0).unwrap();
        assert_eq!(p.root, expected_root, "root");
        for i in 0..MIMC_TREE_HEIGHT {
            assert_eq!(p.path_elements[i], MIMC_ZEROS[i], "sibling[{i}]");
            assert_eq!(p.path_indices[i], 0, "index[{i}]");
        }
        assert_eq!(root_from_path(&leaf, &p), expected_root);
    }

    #[test]
    fn multi_leaf_path_rebuilds_to_the_tree_root_for_every_index() {
        let leaves: Vec<BigUint> = (1..=5u32).map(|i| mimc1(&BigUint::from(i * 7919)).unwrap()).collect();

        let root = mimc_path_from_leaves(&leaves, 0).unwrap().root;
        for idx in 0..leaves.len() {
            let p = mimc_path_from_leaves(&leaves, idx).unwrap();
            assert_eq!(p.root, root, "root stable across index {idx}");
            assert_eq!(root_from_path(&leaves[idx], &p), root, "walk-back idx {idx}");
        }
    }

    #[test]
    fn find_leaf_index_locates_and_rejects() {
        let leaves = vec![mimc1(&BigUint::from(1u32)).unwrap(), mimc1(&BigUint::from(2u32)).unwrap()];
        assert_eq!(mimc_find_leaf_index(&leaves, &mimc1(&BigUint::from(2u32)).unwrap()), Some(1));
        assert_eq!(mimc_find_leaf_index(&leaves, &BigUint::from(999u32)), None);
    }
}
