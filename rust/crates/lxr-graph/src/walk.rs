//! Walking the graph: what a change reaches, how two nodes connect, and the busiest nodes.

use crate::types::*;
use std::collections::VecDeque;

impl Graph {
    /// Every node a change to `node` can reach within `depth` steps, nearest first.
    ///
    /// The walk goes backwards over `Calls` (the callers, then their callers) and over
    /// `Inherits` (the subclasses). Each node is reported once, at its smallest depth.
    /// Everything `node` holds, nested at any depth, is a starting point as well but is not
    /// reported itself. For a file node the files that import it are reported at depth 1.
    /// Ordered by (depth, path, start line).
    pub fn affected(&self, node: u32, depth: u32) -> Vec<Affected> {
        let total = self.nodes.len();
        if node as usize >= total || depth == 0 {
            return Vec::new();
        }
        let mut seen = vec![false; total];
        seen[node as usize] = true;
        let mut frontier = vec![node];
        let mut at = 0;
        while at < frontier.len() {
            let current = frontier[at];
            at += 1;
            for member in self.members(current) {
                if !seen[member as usize] {
                    seen[member as usize] = true;
                    frontier.push(member);
                }
            }
        }
        let start_is_file = matches!(self.nodes[node as usize].kind, NodeKind::File);
        let mut found: Vec<Affected> = Vec::new();
        let mut level = 0;
        while level < depth && !frontier.is_empty() {
            frontier.sort_unstable_by_key(|&n| self.order_key(n));
            let mut next = Vec::new();
            for &current in &frontier {
                for &ei in self.tables.inc.row(current) {
                    let edge = &self.edges[ei as usize];
                    let walks = match edge.rel {
                        Relation::Calls | Relation::Inherits => true,
                        Relation::Imports => start_is_file && level == 0 && current == node,
                        _ => false,
                    };
                    if !walks || edge.from == current || seen[edge.from as usize] {
                        continue;
                    }
                    seen[edge.from as usize] = true;
                    found.push(Affected {
                        node: edge.from,
                        depth: level + 1,
                        via: edge.rel,
                        line: edge.lines.first().copied().unwrap_or(0),
                    });
                    next.push(edge.from);
                }
            }
            frontier = next;
            level += 1;
        }
        found.sort_unstable_by_key(|a| (a.depth, self.order_key(a.node)));
        found
    }

    /// The shortest chain of edges between two nodes, taking every edge as undirected.
    /// Neighbours are tried in order of node number, then edge order, so the answer is
    /// always the same. The first hop is `from` itself; each later hop names the edge that
    /// reached it and whether that edge points forward along the path. `None` when the
    /// nodes are not connected.
    pub fn shortest_path(&self, from: u32, to: u32) -> Option<Vec<Hop>> {
        let total = self.nodes.len();
        if from as usize >= total || to as usize >= total {
            return None;
        }
        let first = Hop { node: from, rel: Relation::Contains, forward: true, conf: Confidence::Extracted };
        if from == to {
            return Some(vec![first]);
        }
        let mut reached_by: Vec<(u32, u32)> = vec![(NONE, NONE); total];
        let mut seen = vec![false; total];
        seen[from as usize] = true;
        let mut queue = VecDeque::from([from]);
        'search: while let Some(current) = queue.pop_front() {
            for &ei in self.tables.und.row(current) {
                let edge = &self.edges[ei as usize];
                let other = if edge.from == current { edge.to } else { edge.from };
                if seen[other as usize] {
                    continue;
                }
                seen[other as usize] = true;
                reached_by[other as usize] = (current, ei);
                if other == to {
                    break 'search;
                }
                queue.push_back(other);
            }
        }
        if !seen[to as usize] {
            return None;
        }
        let mut hops = Vec::new();
        let mut at = to;
        while at != from {
            let (before, ei) = reached_by[at as usize];
            let edge = &self.edges[ei as usize];
            hops.push(Hop { node: at, rel: edge.rel, forward: edge.from == before, conf: edge.conf });
            at = before;
        }
        hops.push(first);
        hops.reverse();
        Some(hops)
    }

    /// The most connected symbols, with their number of distinct neighbours over `Calls`,
    /// `Inherits`, `Method` and `Contains` in both directions. File nodes are left out.
    /// Equal counts are ordered by (path, start line). At most `limit` are returned.
    pub fn hubs(&self, limit: usize) -> Vec<(u32, usize)> {
        let counted = |rel: Relation| matches!(rel, Relation::Calls | Relation::Inherits | Relation::Method | Relation::Contains);
        let mut scored: Vec<(u32, usize)> = Vec::new();
        for (id, node) in self.nodes.iter().enumerate() {
            if !matches!(node.kind, NodeKind::Symbol(_)) {
                continue;
            }
            let mut last = NONE;
            let mut count = 0;
            for &ei in self.tables.und.row(id as u32) {
                let edge = &self.edges[ei as usize];
                if !counted(edge.rel) {
                    continue;
                }
                let other = if edge.from as usize == id { edge.to } else { edge.from };
                if other != last {
                    count += 1;
                    last = other;
                }
            }
            scored.push((id as u32, count));
        }
        scored.sort_unstable_by_key(|&(id, count)| (std::cmp::Reverse(count), self.order_key(id)));
        scored.truncate(limit);
        scored
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::*;
    use crate::{Affected, Graph, Hop};

    fn reach(g: &Graph, found: &[Affected]) -> Vec<String> {
        found.iter().map(|a| format!("{} d{} {:?} L{}", label(g, a.node), a.depth, a.via, a.line)).collect()
    }

    fn route(g: &Graph, hops: &[Hop]) -> Vec<String> {
        hops.iter()
            .enumerate()
            .map(|(i, h)| {
                if i == 0 {
                    label(g, h.node)
                } else {
                    format!("{} {:?} {} {:?}", label(g, h.node), h.rel, if h.forward { "fwd" } else { "back" }, h.conf)
                }
            })
            .collect()
    }

    #[test]
    fn affected_walks_callers_nearest_first_and_ends_at_module_level() {
        let g = Graph::build(&py_project());
        let parse_args = one(&g, "parse_args");
        assert_eq!(
            reach(&g, &g.affected(parse_args, 3)),
            [
                "app/main.py::run d1 Calls L9",
                "app/services/store.py::open_store d1 Calls L6",
                "app/util.py::app/util.py d1 Calls L30",
                "app/main.py::app/main.py d2 Calls L30",
            ]
        );
        assert_eq!(reach(&g, &g.affected(parse_args, 1)).len(), 3);
        assert_eq!(reach(&g, &g.affected(parse_args, 2)).len(), 4);
        assert!(g.affected(parse_args, 0).is_empty());
        assert!(g.affected(u32::MAX, 3).is_empty());
        // nothing calls fact except itself, and recursion does not count
        assert!(g.affected(one(&g, "fact"), 5).is_empty());
    }

    #[test]
    fn affected_takes_subclasses_and_the_members_of_the_start_node() {
        let g = Graph::build(&py_project());
        // Base: the callers of save, validate and make, and the subclass User at depth 1;
        // User's own subclasses and the module-level caller of run at depth 2
        assert_eq!(
            reach(&g, &g.affected(one(&g, "Base"), 3)),
            [
                "app/main.py::run d1 Calls L24",
                "app/models.py::User d1 Inherits L12",
                "app/models.py::User.update d1 Calls L14",
                "app/models.py::User.create d1 Calls L21",
                "app/models.py::Admin.update d1 Calls L30",
                "app/models.py::Admin.run d1 Calls L33",
                "app/services/store.py::open_store d1 Calls L10",
                "app/extras.py::Guest d2 Inherits L3",
                "app/main.py::app/main.py d2 Calls L30",
                "app/models.py::Admin d2 Inherits L27",
            ]
        );
        assert_eq!(reach(&g, &g.affected(one(&g, "Base"), 1)).len(), 7);
        // the members of User are start points that are not reported themselves
        assert_eq!(
            reach(&g, &g.affected(one(&g, "User"), 1)),
            [
                "app/extras.py::Guest d1 Inherits L3",
                "app/extras.py::Guest.greet d1 Calls L5",
                "app/main.py::run d1 Calls L21",
                "app/models.py::Admin d1 Inherits L27",
                "app/models.py::Admin.update d1 Calls L29",
                "app/services/store.py::open_store d1 Calls L7",
            ]
        );
    }

    #[test]
    fn affected_of_a_file_adds_its_importers_and_the_callers_of_everything_in_it() {
        let g = Graph::build(&py_project());
        assert_eq!(
            reach(&g, &g.affected(one(&g, "app/util.py"), 2)),
            [
                "app/main.py::app/main.py d1 Imports L1",
                "app/main.py::run d1 Calls L9",
                "app/services/store.py::app/services/store.py d1 Imports L1",
                "app/services/store.py::open_store d1 Calls L6",
            ]
        );
    }

    #[test]
    fn shortest_path_reports_each_edge_and_its_direction() {
        let g = Graph::build(&py_project());
        let path = |a: &str, b: &str| g.shortest_path(one(&g, a), one(&g, b)).map(|h| route(&g, &h));
        assert_eq!(
            path("Admin.run", "Base"),
            Some(vec![
                "app/models.py::Admin.run".to_string(),
                "app/models.py::Base.validate Calls fwd Extracted".to_string(),
                "app/models.py::Base Method back Extracted".to_string(),
            ])
        );
        assert_eq!(
            path("app/main.py::run", "Base.make"),
            Some(vec![
                "app/main.py::run".to_string(),
                "app/models.py::User.create Calls fwd Extracted".to_string(),
                "app/models.py::Base.make Calls fwd Extracted".to_string(),
            ])
        );
        // against the direction of two Inherits edges
        assert_eq!(
            path("Base", "Guest"),
            Some(vec![
                "app/models.py::Base".to_string(),
                "app/models.py::User Inherits back Extracted".to_string(),
                "app/extras.py::Guest Inherits back Extracted".to_string(),
            ])
        );
        // a file to what it contains, and an inferred edge keeps its confidence
        assert_eq!(
            path("app/helpers.py", "slugify"),
            Some(vec!["app/helpers.py::app/helpers.py".to_string(), "app/helpers.py::slugify Contains fwd Extracted".to_string()])
        );
        assert_eq!(
            path("app/main.py::run", "unique_fn").unwrap()[1],
            "tools/format.py::unique_fn Calls fwd Inferred"
        );
    }

    #[test]
    fn shortest_path_picks_the_same_route_among_equals() {
        let g = Graph::build(&py_project());
        // User.update to Base has two routes of two hops: through User and through Base.save.
        // Neighbours are tried by node number, and Base.save comes before User.
        let hops = g.shortest_path(one(&g, "User.update"), one(&g, "Base")).unwrap();
        assert_eq!(
            route(&g, &hops),
            [
                "app/models.py::User.update",
                "app/models.py::Base.save Calls fwd Extracted",
                "app/models.py::Base Method back Extracted",
            ]
        );
        for _ in 0..3 {
            assert_eq!(g.shortest_path(one(&g, "User.update"), one(&g, "Base")).unwrap(), hops);
        }
    }

    #[test]
    fn shortest_path_handles_same_node_no_route_and_bad_nodes() {
        let g = Graph::build(&py_project());
        let base = one(&g, "Base");
        let same = g.shortest_path(base, base).unwrap();
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].node, base);
        assert!(g.shortest_path(one(&g, "pkg_a/x.py::fmt"), base).is_none());
        assert!(g.shortest_path(base, u32::MAX).is_none());
        assert!(g.shortest_path(u32::MAX, base).is_none());
    }

    #[test]
    fn hubs_count_distinct_neighbours_and_leave_files_out() {
        let g = Graph::build(&py_project());
        let top: Vec<(String, usize)> = g.hubs(5).into_iter().map(|(n, d)| (label(&g, n), d)).collect();
        assert_eq!(
            top,
            [
                ("app/main.py::run".to_string(), 11),
                ("app/models.py::User".to_string(), 10),
                ("app/services/store.py::open_store".to_string(), 7),
                ("app/models.py::Base".to_string(), 5),
                ("app/models.py::Base.save".to_string(), 5),
            ]
        );
        assert!(g.hubs(0).is_empty());
        let all = g.hubs(1000);
        assert_eq!(all.len(), 41);
        assert!(all.iter().all(|&(n, _)| n >= 15));
        assert!(all.windows(2).all(|w| w[0].1 >= w[1].1));
        // recursion adds no neighbour
        assert_eq!(all.iter().find(|&&(n, _)| n == one(&g, "fact")).map(|x| x.1), Some(1));
    }
}
