/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The checked binary decision diagram and its wire encoding.
//!
//! This module only represents and validates the graph. It never tests conditions.

use std::fmt;

/// Encoded references at or above this value address results.
const RESULT_OFFSET: i32 = 100_000_000;
/// Bytes per wire node: three big-endian `i32`s.
const WIRE_NODE_BYTES: usize = 12;
/// The terminal sentinel stored as wire node 0.
const SENTINEL: [i32; 3] = [-1, 1, -1];

/// A checked endpoint decision diagram with the wire sentinel removed.
///
/// Every reference in a `Bdd` addresses an existing node or result, every node's condition
/// index is in range, and every edge between decision nodes moves to a strictly greater
/// condition index. The last property means the graph is acyclic and a path tests each
/// condition at most once.
///
/// # Traversal contract
///
/// This crate does not traverse a `Bdd` with request values. A downstream evaluator should:
///
/// 1. start at [`root`](Self::root);
/// 2. for [`BddRef::Node`], evaluate [`BddNode::condition`] and follow
///    [`high`](BddNode::high) when `condition_value ^ reference.is_complemented()` is `true`,
///    otherwise [`low`](BddNode::low);
/// 3. treat [`BddRef::Terminal`] (either value) as "no endpoint rule matched"; and
/// 4. for [`BddRef::Result`], use [`BddResultRef::serialized_index`] to find the rule, where
///    `None` (the implicit no-match result) also means no match.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Bdd {
    root: BddRef,
    nodes: Box<[BddNode]>,
    condition_count: u32,
    result_count: u32,
}

/// A reference to a terminal, decision node, or result.
///
/// This enum is exhaustive: it is the complete reference taxonomy of the wire format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BddRef {
    /// A boolean terminal. Both values mean that no endpoint rule matched.
    Terminal(bool),
    /// A decision node, possibly through a complemented edge.
    Node(BddNodeRef),
    /// A result.
    Result(BddResultRef),
}

/// A reference to a decision node of a [`Bdd`].
///
/// References can only be obtained from a checked `Bdd`, so [`Bdd::node`] cannot fail for a
/// reference obtained from the same diagram.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BddNodeRef {
    /// Index into the sentinel-free node slice: encoded `|reference| - 2`.
    index: u32,
    complemented: bool,
}

impl BddNodeRef {
    /// The index into [`Bdd::nodes`].
    pub fn index(self) -> usize {
        self.index as usize
    }

    /// Whether the edge is complemented, inverting branch selection.
    pub fn is_complemented(self) -> bool {
        self.complemented
    }

    /// The same node through an uncomplemented edge.
    pub fn without_complement(self) -> Self {
        Self {
            complemented: false,
            ..self
        }
    }

    fn encode(self) -> i32 {
        // Checked during construction: `index + 2 < RESULT_OFFSET`.
        let reference = self.index as i32 + 2;
        if self.complemented {
            -reference
        } else {
            reference
        }
    }
}

/// A reference to a result in the reference implementation's in-memory index space.
///
/// Index 0 is the implicit no-match result, which is not serialized. Index `n > 0` is
/// serialized result `n - 1`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct BddResultRef(u32);

impl BddResultRef {
    /// The implicit no-match result.
    pub const NO_MATCH: Self = Self(0);

    /// Whether this is the implicit no-match result.
    pub fn is_no_match(self) -> bool {
        self.0 == 0
    }

    /// The in-memory index, where 0 is the implicit no-match result.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// The index into the serialized results, or `None` for the implicit no-match result.
    pub fn serialized_index(self) -> Option<usize> {
        (self.0 as usize).checked_sub(1)
    }
}

/// A decision node: test a condition, then follow the high or low reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BddNode {
    condition: u32,
    high: BddRef,
    low: BddRef,
}

impl BddNode {
    /// The index of the tested condition.
    pub fn condition(&self) -> usize {
        self.condition as usize
    }

    /// The reference followed when the (possibly complemented) condition is true.
    pub fn high(&self) -> BddRef {
        self.high
    }

    /// The reference followed when the (possibly complemented) condition is false.
    pub fn low(&self) -> BddRef {
        self.low
    }
}

/// A [`Bdd`] could not be constructed because its data is malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidBddError {
    message: String,
    at_root: bool,
}

impl InvalidBddError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            at_root: false,
        }
    }

    /// Whether the error is about the root reference rather than the node payload.
    pub(crate) fn is_at_root(&self) -> bool {
        self.at_root
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidBddError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InvalidBddError {}

/// Where a reference appears, for error messages.
#[derive(Clone, Copy)]
enum Site {
    Root,
    Field { node: usize, field: &'static str },
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Site::Root => f.write_str("`root`"),
            Site::Field { node, field } => write!(f, "wire node {node} {field}"),
        }
    }
}

impl Bdd {
    /// Builds a checked diagram from encoded wire references.
    ///
    /// `nodes` are the wire nodes **after** the sentinel, each
    /// `[condition_index, high_reference, low_reference]`. References use the wire encoding:
    /// `±1` terminals, `±2..` decision nodes (negative for a complemented edge), and
    /// `100_000_000 + i` result `i`, where result 0 is the implicit no-match result.
    /// `result_count` includes the implicit no-match result.
    ///
    /// Returns an error for any invalid reference or condition index, a complemented
    /// decision-node root or result reference, or an edge whose child decision node does not
    /// test a strictly greater condition index than its parent.
    pub fn from_encoded(
        root: i32,
        nodes: &[[i32; 3]],
        condition_count: usize,
        result_count: usize,
    ) -> Result<Self, InvalidBddError> {
        let condition_count = u32::try_from(condition_count)
            .map_err(|_| InvalidBddError::new("too many conditions"))?;
        let result_count = u32::try_from(result_count)
            .ok()
            .filter(|count| (1..=(i32::MAX - RESULT_OFFSET) as u32 + 1).contains(count))
            .ok_or_else(|| {
                InvalidBddError::new("result count must include the implicit no-match result")
            })?;
        // Every decision-node reference must stay below `RESULT_OFFSET`.
        if nodes.len() > (RESULT_OFFSET - 2) as usize {
            return Err(InvalidBddError::new(format!(
                "{} decision nodes exceed the encodable maximum of {}",
                nodes.len(),
                RESULT_OFFSET - 2
            )));
        }
        let classify = |reference: i32, site: Site| {
            classify(reference, site, nodes.len() as u32, result_count)
        };

        let root = classify(root, Site::Root)?;
        if let BddRef::Node(node) = root {
            if node.complemented {
                return Err(InvalidBddError {
                    at_root: true,
                    ..InvalidBddError::new("`root` must not be a complemented node reference")
                });
            }
        }

        let mut typed = Vec::with_capacity(nodes.len());
        for (index, &[condition, high, low]) in nodes.iter().enumerate() {
            // Wire node 0 is the sentinel.
            let wire = index + 1;
            let condition = u32::try_from(condition)
                .ok()
                .filter(|c| *c < condition_count)
                .ok_or_else(|| {
                    InvalidBddError::new(format!(
                        "wire node {wire} condition index {condition} is out of range for {condition_count} conditions"
                    ))
                })?;
            typed.push(BddNode {
                condition,
                high: classify(
                    high,
                    Site::Field {
                        node: wire,
                        field: "high reference",
                    },
                )?,
                low: classify(
                    low,
                    Site::Field {
                        node: wire,
                        field: "low reference",
                    },
                )?,
            });
        }

        // Strict variable ordering. Because condition indexes strictly increase along every
        // edge, this also rejects every cycle, including self-references, without recursion.
        for (index, node) in typed.iter().enumerate() {
            for (field, child) in [("high", node.high), ("low", node.low)] {
                if let BddRef::Node(child) = child {
                    let child_condition = typed[child.index()].condition;
                    if child_condition <= node.condition {
                        return Err(InvalidBddError::new(format!(
                            "wire node {} {field} edge leads to wire node {}, whose condition {child_condition} is not greater than {}",
                            index + 1,
                            child.index() + 1,
                            node.condition
                        )));
                    }
                }
            }
        }

        Ok(Self {
            root,
            nodes: typed.into_boxed_slice(),
            condition_count,
            result_count,
        })
    }

    /// Decodes the big-endian wire payload, including the leading sentinel.
    ///
    /// `bytes.len()` must be exactly `wire_node_count * 12`.
    pub(crate) fn from_wire_bytes(
        root: i32,
        wire_node_count: usize,
        bytes: &[u8],
        condition_count: usize,
        result_count: usize,
    ) -> Result<Self, InvalidBddError> {
        let expected = wire_byte_len(wire_node_count)
            .ok_or_else(|| InvalidBddError::new("`nodeCount` is too large"))?;
        if bytes.len() != expected {
            return Err(InvalidBddError::new(format!(
                "`nodes` decodes to {} bytes, but `nodeCount` {wire_node_count} requires {expected}",
                bytes.len()
            )));
        }
        if wire_node_count == 0 {
            return Err(InvalidBddError::new(
                "`nodeCount` must be at least 1 to include the terminal sentinel",
            ));
        }
        let mut nodes = bytes.chunks_exact(WIRE_NODE_BYTES).map(|chunk| {
            let int = |i: usize| i32::from_be_bytes(chunk[i..i + 4].try_into().expect("4 bytes"));
            [int(0), int(4), int(8)]
        });
        let sentinel = nodes.next().expect("at least one node");
        if sentinel != SENTINEL {
            return Err(InvalidBddError::new(format!(
                "wire node 0 must be the terminal sentinel {SENTINEL:?}, found {sentinel:?}"
            )));
        }
        let nodes: Vec<_> = nodes.collect();
        Self::from_encoded(root, &nodes, condition_count, result_count)
    }

    /// Encodes the wire payload, including the leading sentinel, as big-endian bytes.
    pub(crate) fn to_wire_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.wire_node_count() * WIRE_NODE_BYTES);
        let mut push = |values: [i32; 3]| {
            for value in values {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
        };
        push(SENTINEL);
        for node in self.nodes.iter() {
            push([node.condition as i32, encode(node.high), encode(node.low)]);
        }
        bytes
    }

    /// The encoded root reference.
    pub(crate) fn encoded_root(&self) -> i32 {
        encode(self.root)
    }

    /// The reference at which traversal starts.
    pub fn root(&self) -> BddRef {
        self.root
    }

    /// The decision nodes in storage order, without the wire sentinel.
    pub fn nodes(&self) -> &[BddNode] {
        &self.nodes
    }

    /// The node addressed by `reference`, ignoring any complement.
    ///
    /// # Panics
    ///
    /// Panics if `reference` was obtained from a different, smaller diagram.
    pub fn node(&self, reference: BddNodeRef) -> &BddNode {
        &self.nodes[reference.index()]
    }

    /// The wire node count: decision nodes plus the terminal sentinel.
    pub fn wire_node_count(&self) -> usize {
        self.nodes.len() + 1
    }

    /// The number of conditions that node condition indexes address.
    pub fn condition_count(&self) -> usize {
        self.condition_count as usize
    }

    /// The number of results, including the implicit no-match result.
    pub fn result_count(&self) -> usize {
        self.result_count as usize
    }

    /// The decision nodes reachable from the root, in depth-first preorder (high before low).
    ///
    /// Both branches of every node are followed; no condition is tested. Each node is
    /// yielded once, through an uncomplemented reference.
    pub fn reachable_nodes(&self) -> impl Iterator<Item = (BddNodeRef, &BddNode)> + '_ {
        let mut visited = vec![false; self.nodes.len()];
        let mut stack = Vec::new();
        let mut order = Vec::new();
        let mut push = |reference: BddRef, stack: &mut Vec<BddNodeRef>| {
            if let BddRef::Node(node) = reference {
                if !std::mem::replace(&mut visited[node.index()], true) {
                    stack.push(node.without_complement());
                }
            }
        };
        push(self.root, &mut stack);
        while let Some(reference) = stack.pop() {
            order.push(reference);
            let node = self.node(reference);
            // Low is pushed first so high is visited first.
            push(node.low, &mut stack);
            push(node.high, &mut stack);
        }
        order
            .into_iter()
            .map(move |reference| (reference, self.node(reference)))
    }

    /// The distinct condition indexes tested by any stored node, in ascending order.
    pub fn referenced_conditions(&self) -> impl Iterator<Item = usize> + '_ {
        let mut seen = vec![false; self.condition_count()];
        for node in self.nodes.iter() {
            seen[node.condition()] = true;
        }
        seen.into_iter()
            .enumerate()
            .filter_map(|(index, seen)| seen.then_some(index))
    }

    /// The distinct results referenced by the root or any stored node, in ascending order.
    pub fn referenced_results(&self) -> impl Iterator<Item = BddResultRef> + '_ {
        let mut seen = vec![false; self.result_count()];
        let edges = self.nodes.iter().flat_map(|node| [node.high, node.low]);
        for reference in std::iter::once(self.root).chain(edges) {
            if let BddRef::Result(result) = reference {
                seen[result.index()] = true;
            }
        }
        seen.into_iter()
            .enumerate()
            .filter_map(|(index, seen)| seen.then_some(BddResultRef(index as u32)))
    }
}

/// `wire_node_count * 12`, if it fits.
pub(crate) fn wire_byte_len(wire_node_count: usize) -> Option<usize> {
    wire_node_count.checked_mul(WIRE_NODE_BYTES)
}

fn classify(
    reference: i32,
    site: Site,
    node_count: u32,
    result_count: u32,
) -> Result<BddRef, InvalidBddError> {
    let invalid = |reason: String| InvalidBddError {
        at_root: matches!(site, Site::Root),
        ..InvalidBddError::new(format!("{site} {reason}"))
    };
    match reference {
        0 => Err(invalid("is the invalid reference 0".into())),
        1 => Ok(BddRef::Terminal(true)),
        -1 => Ok(BddRef::Terminal(false)),
        r if r >= RESULT_OFFSET => {
            let index = (r - RESULT_OFFSET) as u32;
            if index < result_count {
                Ok(BddRef::Result(BddResultRef(index)))
            } else {
                Err(invalid(format!(
                    "references result {index}, but there are {result_count} results including no-match"
                )))
            }
        }
        r if r <= -RESULT_OFFSET => {
            Err(invalid(format!("is a complemented result reference ({r})")))
        }
        r => {
            // `r` is in `2..RESULT_OFFSET` or `-RESULT_OFFSET+1..=-2`, so `unsigned_abs` is exact.
            let index = r.unsigned_abs() - 2;
            if index < node_count {
                Ok(BddRef::Node(BddNodeRef {
                    index,
                    complemented: r < 0,
                }))
            } else {
                Err(invalid(format!(
                    "references wire node {}, but there are {} wire nodes",
                    index + 1,
                    node_count + 1
                )))
            }
        }
    }
}

fn encode(reference: BddRef) -> i32 {
    match reference {
        BddRef::Terminal(true) => 1,
        BddRef::Terminal(false) => -1,
        BddRef::Node(node) => node.encode(),
        // Checked during construction: `index < result_count <= i32::MAX - RESULT_OFFSET + 1`.
        BddRef::Result(result) => RESULT_OFFSET + result.0 as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: i32 = RESULT_OFFSET;

    fn bdd(root: i32, nodes: &[[i32; 3]]) -> Result<Bdd, InvalidBddError> {
        Bdd::from_encoded(root, nodes, 4, 3)
    }

    fn err(root: i32, nodes: &[[i32; 3]]) -> String {
        bdd(root, nodes).unwrap_err().to_string()
    }

    #[test]
    fn classifies_every_reference_kind() {
        let graph = bdd(2, &[[0, 3, -1], [1, R + 2, -4], [2, 1, R]]).unwrap();
        let [a, b, c] = [0, 1, 2].map(|i| BddNodeRef {
            index: i,
            complemented: false,
        });
        assert_eq!(graph.root(), BddRef::Node(a));
        assert_eq!(graph.node(a).high(), BddRef::Node(b));
        assert_eq!(graph.node(a).low(), BddRef::Terminal(false));
        assert_eq!(graph.node(b).high(), BddRef::Result(BddResultRef(2)));
        let complemented_c = BddRef::Node(BddNodeRef {
            complemented: true,
            ..c
        });
        assert_eq!(graph.node(b).low(), complemented_c);
        assert_ne!(graph.node(b).low(), BddRef::Node(c));
        assert_eq!(graph.node(c).high(), BddRef::Terminal(true));
        assert_eq!(graph.node(c).low(), BddRef::Result(BddResultRef::NO_MATCH));
        assert_eq!(graph.wire_node_count(), 4);
        assert_eq!(graph.condition_count(), 4);
        assert_eq!(graph.result_count(), 3);
    }

    #[test]
    fn complemented_edges_keep_node_identity() {
        let graph = bdd(2, &[[0, -3, 3], [1, 1, -1]]).unwrap();
        let BddRef::Node(high) = graph
            .node(BddNodeRef {
                index: 0,
                complemented: false,
            })
            .high()
        else {
            panic!()
        };
        assert!(high.is_complemented());
        assert_eq!(high.index(), 1);
        assert!(!high.without_complement().is_complemented());
        assert_eq!(graph.node(high), graph.node(high.without_complement()));
    }

    #[test]
    fn result_refs_shift_for_the_implicit_no_match() {
        assert!(BddResultRef::NO_MATCH.is_no_match());
        assert_eq!(BddResultRef::NO_MATCH.serialized_index(), None);
        assert_eq!(BddResultRef(1).serialized_index(), Some(0));
        assert_eq!(BddResultRef(7).serialized_index(), Some(6));
    }

    #[test]
    fn terminal_and_result_roots_are_valid() {
        for root in [1, -1, R, R + 2] {
            let graph = bdd(root, &[]).unwrap();
            assert_eq!(graph.encoded_root(), root);
            assert_eq!(graph.reachable_nodes().count(), 0);
        }
    }

    #[test]
    fn rejects_invalid_references() {
        let at_root = |r| err(r, &[[0, 1, -1]]);
        assert!(at_root(0).contains("`root` is the invalid reference 0"));
        assert!(at_root(i32::MIN).contains("complemented result"));
        assert!(at_root(-R).contains("complemented result"));
        assert!(at_root(-2).contains("must not be a complemented node"));
        assert!(at_root(3).contains("references wire node 2, but there are 2"));
        assert!(at_root(R + 3).contains("references result 3"));
        assert!(at_root(R - 1).contains("references wire node 99999998"));
        assert!(at_root(-(R - 1)).contains("references wire node 99999998"));
        assert!(err(2, &[[0, 0, 1]]).contains("wire node 1 high reference is the invalid"));
        assert!(err(2, &[[0, 1, -R - 1]]).contains("wire node 1 low reference"));
    }

    #[test]
    fn rejects_invalid_condition_indexes() {
        assert!(err(2, &[[4, 1, -1]]).contains("condition index 4 is out of range"));
        assert!(err(2, &[[-1, 1, -1]]).contains("condition index -1 is out of range"));
    }

    #[test]
    fn rejects_cycles_and_unordered_conditions() {
        assert!(err(2, &[[0, 2, -1]]).contains("wire node 1 high edge leads to wire node 1"));
        assert!(err(2, &[[0, 3, -1], [1, -1, -2]])
            .contains("wire node 2 low edge leads to wire node 1"));
        // Repeated and descending conditions.
        assert!(err(2, &[[1, 3, -1], [1, 1, -1]]).contains("is not greater than 1"));
        assert!(err(2, &[[2, 3, -1], [1, 1, -1]]).contains("condition 1 is not greater than 2"));
        // Unreachable malformed nodes are still rejected.
        assert!(err(1, &[[0, 0, 1]]).contains("invalid reference 0"));
    }

    #[test]
    fn rejects_counts_outside_the_encoding() {
        assert!(Bdd::from_encoded(1, &[], 0, 0).is_err());
        assert!(Bdd::from_encoded(1, &[], 0, usize::MAX).is_err());
        let max = (i32::MAX - R) as usize + 1;
        let graph = Bdd::from_encoded(i32::MAX, &[], 0, max).unwrap();
        assert_eq!(graph.encoded_root(), i32::MAX);
    }

    #[test]
    fn wire_bytes_round_trip() {
        let graph = bdd(2, &[[0, 3, R + 1], [1, -1, R + 2]]).unwrap();
        let bytes = graph.to_wire_bytes();
        assert_eq!(bytes.len(), 36);
        assert_eq!(
            &bytes[..12],
            &[255, 255, 255, 255, 0, 0, 0, 1, 255, 255, 255, 255]
        );
        let decoded = Bdd::from_wire_bytes(2, 3, &bytes, 4, 3).unwrap();
        assert_eq!(decoded, graph);
    }

    #[test]
    fn wire_bytes_validate_length_and_sentinel() {
        let bytes = bdd(1, &[]).unwrap().to_wire_bytes();
        assert!(Bdd::from_wire_bytes(1, 1, &bytes, 0, 1).is_ok());
        let e = Bdd::from_wire_bytes(1, 2, &bytes, 0, 1).unwrap_err();
        assert!(e.message().contains("requires 24"), "{e}");
        let e = Bdd::from_wire_bytes(1, 0, &[], 0, 1).unwrap_err();
        assert!(e.message().contains("at least 1"), "{e}");
        let e = Bdd::from_wire_bytes(1, usize::MAX, &bytes, 0, 1).unwrap_err();
        assert!(e.message().contains("too large"), "{e}");
        let mut changed = bytes.clone();
        changed[3] = 0xfe;
        let e = Bdd::from_wire_bytes(1, 1, &changed, 0, 1).unwrap_err();
        assert!(e.message().contains("terminal sentinel"), "{e}");
    }

    #[test]
    fn inspection_is_deterministic_and_complete() {
        // 0 -> (1, 2); 1 -> (3, R1); 2 -> (3, T); 3 -> (R2, NoMatch); 4 is unreachable.
        let graph = Bdd::from_encoded(
            2,
            &[
                [0, 3, -4],
                [1, 5, R + 1],
                [2, 5, 1],
                [3, R + 2, R],
                [3, R + 3, R + 3],
            ],
            5,
            4,
        )
        .unwrap();
        let reachable: Vec<_> = graph
            .reachable_nodes()
            .map(|(reference, node)| {
                assert!(!reference.is_complemented());
                assert_eq!(graph.node(reference), node);
                reference.index()
            })
            .collect();
        assert_eq!(reachable, [0, 1, 3, 2]);
        assert_eq!(
            graph.referenced_conditions().collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            graph
                .referenced_results()
                .map(BddResultRef::index)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        let root_only = Bdd::from_encoded(R + 1, &[[0, 1, -1]], 2, 2).unwrap();
        assert_eq!(root_only.reachable_nodes().count(), 0);
        assert_eq!(root_only.referenced_conditions().collect::<Vec<_>>(), [0]);
        assert_eq!(
            root_only.referenced_results().collect::<Vec<_>>(),
            [BddResultRef(1)]
        );
    }

    #[test]
    fn deep_chains_do_not_recurse() {
        let count = 100_000;
        let nodes: Vec<_> = (0..count)
            .map(|i| {
                let next = if i + 1 == count { 1 } else { i + 3 };
                [i, next, -1]
            })
            .collect();
        let graph = Bdd::from_encoded(2, &nodes, count as usize, 1).unwrap();
        assert_eq!(graph.reachable_nodes().count(), count as usize);
        let decoded = Bdd::from_wire_bytes(
            2,
            graph.wire_node_count(),
            &graph.to_wire_bytes(),
            count as usize,
            1,
        )
        .unwrap();
        assert_eq!(decoded, graph);
    }
}
