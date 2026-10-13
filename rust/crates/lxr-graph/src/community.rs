//! Grouping symbols into communities by modularity, with the Louvain method.
//! Nothing here is random: nodes are visited in a fixed order and ties go to the
//! lower community number, so the same graph always gives the same groups.

use crate::types::*;

/// Gains closer than this are ties.
const EPSILON: f64 = 1e-12;

/// A weighted undirected graph at one level of the method.
struct Level {
    /// Neighbours of each node with the weight of the edge. Each edge is listed at both ends.
    adj: Vec<Vec<(u32, f64)>>,
    /// Weight of the edges that start and end in the same node.
    loops: Vec<f64>,
}

impl Level {
    /// The weighted degree of a node. A loop counts twice.
    fn degree(&self, node: usize) -> f64 {
        self.adj[node].iter().map(|&(_, w)| w).sum::<f64>() + 2.0 * self.loops[node]
    }
}

/// Moves nodes between communities while that raises modularity. Returns the community of
/// each node, numbered from 0 by first appearance, and whether any node moved.
fn local_moves(level: &Level, resolution: f64) -> (Vec<u32>, bool) {
    let n = level.adj.len();
    let degree: Vec<f64> = (0..n).map(|i| level.degree(i)).collect();
    let two_m: f64 = degree.iter().sum();
    let mut community: Vec<u32> = (0..n as u32).collect();
    if two_m <= 0.0 {
        return (community, false);
    }
    let mut total: Vec<f64> = degree.clone();
    let mut weight_to = vec![0.0f64; n];
    let mut touched: Vec<u32> = Vec::new();
    let mut moved_any = false;
    for _ in 0..100 {
        let mut moved = false;
        for i in 0..n {
            let home = community[i];
            touched.clear();
            for &(j, w) in &level.adj[i] {
                if j as usize == i {
                    continue;
                }
                let c = community[j as usize];
                if weight_to[c as usize] == 0.0 {
                    touched.push(c);
                }
                weight_to[c as usize] += w;
            }
            total[home as usize] -= degree[i];
            let gain = |c: u32, weight: f64| weight - resolution * total[c as usize] * degree[i] / two_m;
            let mut best = home;
            let mut best_gain = gain(home, weight_to[home as usize]);
            for &c in &touched {
                let g = gain(c, weight_to[c as usize]);
                if g > best_gain + EPSILON || ((g - best_gain).abs() <= EPSILON && best != home && c < best) {
                    best = c;
                    best_gain = g;
                }
            }
            total[best as usize] += degree[i];
            community[i] = best;
            if best != home {
                moved = true;
            }
            weight_to[home as usize] = 0.0;
            for &c in &touched {
                weight_to[c as usize] = 0.0;
            }
        }
        if !moved {
            break;
        }
        moved_any = true;
    }
    let mut number = vec![u32::MAX; n];
    let mut next = 0;
    for c in community.iter_mut() {
        if number[*c as usize] == u32::MAX {
            number[*c as usize] = next;
            next += 1;
        }
        *c = number[*c as usize];
    }
    (community, moved_any)
}

/// Collapses each community into one node of the next level.
fn collapse(level: &Level, community: &[u32]) -> Level {
    let groups = community.iter().copied().max().map_or(0, |m| m as usize + 1);
    let mut loops = vec![0.0f64; groups];
    let mut pairs: std::collections::BTreeMap<(u32, u32), f64> = std::collections::BTreeMap::new();
    for (i, edges) in level.adj.iter().enumerate() {
        let ci = community[i];
        loops[ci as usize] += level.loops[i];
        for &(j, w) in edges {
            let cj = community[j as usize];
            if ci == cj {
                // Both ends of an inner edge list it, so each lists half of it.
                loops[ci as usize] += w / 2.0;
            } else if ci < cj {
                *pairs.entry((ci, cj)).or_insert(0.0) += w;
            }
        }
    }
    let mut adj = vec![Vec::new(); groups];
    for ((a, b), w) in pairs {
        adj[a as usize].push((b, w));
        adj[b as usize].push((a, w));
    }
    for row in adj.iter_mut() {
        row.sort_by_key(|&(c, _)| c);
    }
    Level { adj, loops }
}

/// Runs the Louvain method on `n` nodes joined by the unweighted undirected `edges`.
/// Returns the community of each node, numbered from 0 by first appearance.
pub(crate) fn louvain(n: usize, edges: &[(u32, u32)], resolution: f64) -> Vec<u32> {
    let mut adj = vec![Vec::new(); n];
    for &(a, b) in edges {
        if a != b {
            adj[a as usize].push((b, 1.0));
            adj[b as usize].push((a, 1.0));
        }
    }
    for row in adj.iter_mut() {
        row.sort_by_key(|&(c, _)| c);
    }
    let mut level = Level { adj, loops: vec![0.0; n] };
    let mut of_node: Vec<u32> = (0..n as u32).collect();
    loop {
        let (community, moved) = local_moves(&level, resolution);
        if !moved {
            break;
        }
        for c in of_node.iter_mut() {
            *c = community[*c as usize];
        }
        level = collapse(&level, &community);
    }
    of_node
}

impl Graph {
    /// Groups the symbols into communities: Louvain modularity at resolution 1.0 over the
    /// undirected graph of `Calls`, `Inherits`, `Method` and `Contains` between symbols.
    /// Symbols with no such edge are communities of their own. The largest community comes
    /// first; equal sizes are ordered by their lowest node number, and the members of each
    /// community by node number.
    pub fn communities(&self) -> Vec<Vec<u32>> {
        let first = self.files.len();
        let symbols = self.nodes.len().saturating_sub(first);
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for edge in &self.edges {
            let counted = matches!(edge.rel, Relation::Calls | Relation::Inherits | Relation::Method | Relation::Contains);
            let (a, b) = (edge.from as usize, edge.to as usize);
            if !counted || a == b || a < first || b < first || a >= self.nodes.len() || b >= self.nodes.len() {
                continue;
            }
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            pairs.push(((lo - first) as u32, (hi - first) as u32));
        }
        pairs.sort_unstable();
        pairs.dedup();
        let of_symbol = louvain(symbols, &pairs, 1.0);
        let count = of_symbol.iter().copied().max().map_or(0, |m| m as usize + 1);
        let mut groups: Vec<Vec<u32>> = vec![Vec::new(); count];
        for (i, &c) in of_symbol.iter().enumerate() {
            groups[c as usize].push((i + first) as u32);
        }
        groups.sort_by_key(|g| (std::cmp::Reverse(g.len()), g[0]));
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clique(nodes: &[u32]) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for (i, &a) in nodes.iter().enumerate() {
            for &b in &nodes[i + 1..] {
                out.push((a, b));
            }
        }
        out
    }

    #[test]
    fn two_cliques_joined_by_one_edge_make_two_communities() {
        let mut edges = clique(&[0, 1, 2, 3]);
        edges.extend(clique(&[4, 5, 6, 7]));
        edges.push((3, 4));
        let found = louvain(8, &edges, 1.0);
        assert_eq!(found, [0, 0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(louvain(8, &edges, 1.0), found);
    }

    #[test]
    fn isolated_nodes_stay_alone_and_an_empty_graph_is_fine() {
        let found = louvain(5, &[(0, 1)], 1.0);
        assert_eq!(found, [0, 0, 1, 2, 3]);
        assert!(louvain(0, &[], 1.0).is_empty());
        assert_eq!(louvain(3, &[], 1.0), [0, 1, 2]);
        assert_eq!(louvain(2, &[(1, 1)], 1.0), [0, 1]);
    }

    #[test]
    fn a_ring_of_cliques_is_grouped_clique_by_clique() {
        let mut edges = Vec::new();
        for k in 0..4u32 {
            let base = k * 5;
            edges.extend(clique(&[base, base + 1, base + 2, base + 3, base + 4]));
            edges.push((base + 4, ((k + 1) % 4) * 5));
        }
        let found = louvain(20, &edges, 1.0);
        for k in 0..4usize {
            let ids = &found[k * 5..k * 5 + 5];
            assert!(ids.iter().all(|&c| c == ids[0]), "clique {k} split: {ids:?}");
        }
        let mut distinct: Vec<u32> = found.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 4);
    }

    #[test]
    fn symbols_group_into_communities_by_their_calls() {
        let g = Graph::build(&crate::testkit::cluster_project());
        // two groups of four, one call between them, and a function that stands alone
        assert_eq!(g.communities(), vec![vec![3, 4, 5, 6], vec![7, 8, 9, 10], vec![11]]);
    }

    #[test]
    fn every_symbol_is_in_exactly_one_community_largest_first() {
        let files = crate::testkit::py_project();
        let g = Graph::build(&files);
        let groups = g.communities();
        let mut all: Vec<u32> = groups.iter().flatten().copied().collect();
        all.sort_unstable();
        assert_eq!(all, (15..56).collect::<Vec<u32>>());
        assert!(groups.windows(2).all(|w| w[0].len() >= w[1].len()));
        assert!(groups.iter().all(|g| g.windows(2).all(|w| w[0] < w[1])));
        assert_eq!(g.communities(), groups);
        let mut reversed = files.clone();
        reversed.reverse();
        assert_eq!(Graph::build(&reversed).communities(), groups);
    }
}
