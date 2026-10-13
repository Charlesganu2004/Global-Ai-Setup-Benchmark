//! Links between notes: temporal, semantic and causal. Entity overlap is not stored.
//!
//! The links read from `index.bin` stay in the bytes they were read as, laid out so that the
//! links of one note are a run of fixed-size entries: opening a bank does not build 100,000
//! small lists. Links made after that go into a small overlay.

use std::collections::{BTreeSet, HashSet};

use crate::dates::DAY;

/// Most temporal links a new note makes.
pub(crate) const TEMPORAL_LIMIT: usize = 20;
/// Most semantic links a new note makes.
pub(crate) const SEMANTIC_LIMIT: usize = 50;

/// Bytes of one stored link end: the other note (4), the weight (4) and the kind (1).
const ENTRY: usize = 9;

/// What a link says about its two notes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LinkKind {
    /// They are about times less than 24 hours apart.
    Temporal,
    /// Their texts mean nearly the same.
    Semantic,
    /// A cue phrase says one explains the other.
    Causal,
}

impl LinkKind {
    fn code(self) -> u8 {
        match self {
            LinkKind::Temporal => 0,
            LinkKind::Semantic => 1,
            LinkKind::Causal => 2,
        }
    }

    fn from_code(code: u8) -> Option<LinkKind> {
        match code {
            0 => Some(LinkKind::Temporal),
            1 => Some(LinkKind::Semantic),
            2 => Some(LinkKind::Causal),
            _ => None,
        }
    }
}

/// One end of a link, as seen from the other end.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Edge {
    pub to: u32,
    pub kind: LinkKind,
    pub w: f32,
}

/// The links read from a file: `[count][count + 1 run starts][entries]`, all little-endian
/// `u32` except the entries, which are `to`, `weight` and a kind byte.
struct Stored {
    /// The whole file the links were read from.
    bytes: Vec<u8>,
    /// Where the links start in it.
    offset: usize,
    notes: usize,
}

impl Stored {
    fn u32_at(&self, at: usize) -> u32 {
        let b = &self.bytes[self.offset + at..self.offset + at + 4];
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    /// The bytes of the links of note `a`; empty for a note the file does not know.
    fn run(&self, a: u32) -> &[u8] {
        if a as usize >= self.notes {
            return &[];
        }
        let data = 4 + 4 * (self.notes + 1);
        let from = self.offset + data + ENTRY * self.u32_at(4 + 4 * a as usize) as usize;
        let to = self.offset + data + ENTRY * self.u32_at(8 + 4 * a as usize) as usize;
        &self.bytes[from..to]
    }
}

/// Links of all notes, by slot. A link is kept at both of its ends.
#[derive(Default)]
pub(crate) struct Graph {
    stored: Option<Stored>,
    /// Links made since the file was read, by slot.
    extra: Vec<Vec<Edge>>,
    /// Notes taken out since the file was read.
    dead: HashSet<u32>,
}

/// The links of one note.
pub(crate) struct Neighbors<'a> {
    stored: &'a [u8],
    extra: std::slice::Iter<'a, Edge>,
    dead: &'a HashSet<u32>,
}

impl Iterator for Neighbors<'_> {
    type Item = Edge;

    fn next(&mut self) -> Option<Edge> {
        while self.stored.len() >= ENTRY {
            let (entry, rest) = self.stored.split_at(ENTRY);
            self.stored = rest;
            let to = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
            if !self.dead.is_empty() && self.dead.contains(&to) {
                continue;
            }
            let w = f32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
            if let Some(kind) = LinkKind::from_code(entry[8]) {
                return Some(Edge { to, kind, w });
            }
        }
        self.extra.next().copied()
    }
}

impl Graph {
    /// Makes room for `slots` notes.
    pub fn ensure(&mut self, slots: usize) {
        if self.extra.len() < slots {
            self.extra.resize_with(slots, Vec::new);
        }
    }

    /// Links two different notes.
    pub fn add(&mut self, a: u32, b: u32, kind: LinkKind, w: f32) {
        if a == b {
            return;
        }
        self.ensure(a.max(b) as usize + 1);
        self.extra[a as usize].push(Edge { to: b, kind, w });
        self.extra[b as usize].push(Edge { to: a, kind, w });
    }

    /// True when the two notes already have a link of this kind.
    pub fn has(&self, a: u32, b: u32, kind: LinkKind) -> bool {
        self.neighbors(a).any(|e| e.to == b && e.kind == kind)
    }

    /// The links of a note.
    pub fn neighbors(&self, a: u32) -> Neighbors<'_> {
        let alive = !self.dead.contains(&a);
        Neighbors {
            stored: if alive { self.stored.as_ref().map_or(&[][..], |s| s.run(a)) } else { &[] },
            extra: if alive { self.extra.get(a as usize).map_or(&[][..], Vec::as_slice).iter() } else { (&[] as &[Edge]).iter() },
            dead: &self.dead,
        }
    }

    /// Takes every link of a note out of the graph.
    pub fn remove_node(&mut self, a: u32) {
        let others: Vec<u32> = self.neighbors(a).map(|e| e.to).collect();
        for other in others {
            if let Some(list) = self.extra.get_mut(other as usize) {
                list.retain(|e| e.to != a);
            }
        }
        if let Some(list) = self.extra.get_mut(a as usize) {
            list.clear();
        }
        self.dead.insert(a);
    }

    /// Number of links.
    pub fn count(&self) -> usize {
        let ends: usize = (0..self.extra.len().max(self.stored.as_ref().map_or(0, |s| s.notes)) as u32).map(|a| self.neighbors(a).count()).sum();
        ends / 2
    }

    /// The links of `slots` notes in the layout of the file.
    pub fn to_bytes(&self, slots: usize) -> Vec<u8> {
        let mut starts: Vec<u8> = Vec::with_capacity(4 * (slots + 1));
        let mut entries: Vec<u8> = Vec::new();
        let mut total = 0u32;
        starts.extend_from_slice(&total.to_le_bytes());
        for a in 0..slots as u32 {
            for e in self.neighbors(a).filter(|e| (e.to as usize) < slots) {
                entries.extend_from_slice(&e.to.to_le_bytes());
                entries.extend_from_slice(&e.w.to_le_bytes());
                entries.push(e.kind.code());
                total += 1;
            }
            starts.extend_from_slice(&total.to_le_bytes());
        }
        let mut out = Vec::with_capacity(4 + starts.len() + entries.len());
        out.extend_from_slice(&(slots as u32).to_le_bytes());
        out.extend_from_slice(&starts);
        out.extend_from_slice(&entries);
        out
    }

    /// True when `bytes` are links for `slots` notes in the layout of [`Graph::to_bytes`]:
    /// right sizes, runs in order, no link to a note that does not exist, no unknown kind.
    pub fn check(slots: usize, bytes: &[u8]) -> bool {
        let head = 4 + 4 * (slots + 1);
        if bytes.len() < head || (bytes.len() - head) % ENTRY != 0 {
            return false;
        }
        let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        if word(0) as usize != slots || word(4) != 0 {
            return false;
        }
        let entries = (bytes.len() - head) / ENTRY;
        let mut last = 0u32;
        for a in 0..=slots {
            let at = word(4 + 4 * a);
            if at < last {
                return false;
            }
            last = at;
        }
        if last as usize != entries {
            return false;
        }
        bytes[head..].chunks_exact(ENTRY).all(|entry| {
            let to = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
            (to as usize) < slots && entry[8] <= 2
        })
    }

    /// A graph over the links in `file[offset..]`, which [`Graph::check`] accepted for `slots`
    /// notes. The file is kept as it is; nothing is copied.
    pub fn from_checked(slots: usize, file: Vec<u8>, offset: usize) -> Graph {
        let mut graph = Graph { stored: Some(Stored { bytes: file, offset, notes: slots }), ..Graph::default() };
        graph.ensure(slots);
        graph
    }

    /// A graph over the links in `bytes`, as [`Graph::to_bytes`] wrote them for `slots` notes.
    /// `None` when [`Graph::check`] refuses them.
    #[cfg(test)]
    pub fn from_bytes(slots: usize, bytes: Vec<u8>) -> Option<Graph> {
        if Graph::check(slots, &bytes) {
            Some(Graph::from_checked(slots, bytes, 0))
        } else {
            None
        }
    }
}

/// The notes nearest in time to `t`, at most `limit`, none further than 24 hours away, with
/// the link weight `max(0.3, 1 - hours / 24)`. `times` holds `(time, slot)` of the notes that
/// can be linked.
pub(crate) fn temporal_neighbors(times: &BTreeSet<(i64, u32)>, t: i64, limit: usize) -> Vec<(u32, f32)> {
    let mut below = times.range(..(t, 0)).rev().peekable();
    let mut above = times.range((t, 0)..).peekable();
    let mut out = Vec::new();
    while out.len() < limit {
        let before = below.peek().map(|&&(at, _)| t - at);
        let after = above.peek().map(|&&(at, _)| at - t);
        let take_above = match (before, after) {
            (None, None) => break,
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (Some(b), Some(a)) => a <= b,
        };
        let next = if take_above {
            above.next().map(|&(at, slot)| (at - t, slot))
        } else {
            below.next().map(|&(at, slot)| (t - at, slot))
        };
        let Some((distance, slot)) = next else {
            break;
        };
        if distance >= DAY {
            break;
        }
        let hours = distance as f32 / 3600.0;
        out.push((slot, (1.0 - hours / 24.0).max(0.3)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots_of(g: &Graph, a: u32) -> Vec<u32> {
        let mut v: Vec<u32> = g.neighbors(a).map(|e| e.to).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn links_are_kept_at_both_ends_and_counted_once() {
        let mut g = Graph::default();
        g.add(0, 2, LinkKind::Semantic, 0.8);
        g.add(2, 1, LinkKind::Causal, 1.0);
        g.add(1, 1, LinkKind::Temporal, 1.0);
        assert_eq!(g.count(), 2);
        assert_eq!(g.neighbors(2).count(), 2);
        assert!(g.has(0, 2, LinkKind::Semantic) && g.has(2, 0, LinkKind::Semantic));
        assert!(!g.has(0, 2, LinkKind::Causal));
        assert_eq!(g.neighbors(9).count(), 0);
    }

    #[test]
    fn removing_a_node_removes_its_links_from_the_other_ends() {
        let mut g = Graph::default();
        g.add(0, 1, LinkKind::Semantic, 0.7);
        g.add(1, 2, LinkKind::Temporal, 0.5);
        g.remove_node(1);
        assert_eq!(g.count(), 0);
        assert_eq!(g.neighbors(0).count() + g.neighbors(2).count() + g.neighbors(1).count(), 0);
    }

    #[test]
    fn the_byte_form_round_trips_and_rejects_bad_input() {
        let mut g = Graph::default();
        g.add(3, 0, LinkKind::Semantic, 0.75);
        g.add(1, 2, LinkKind::Temporal, 0.5);
        g.add(2, 3, LinkKind::Causal, 1.0);
        let bytes = g.to_bytes(4);
        let again = Graph::from_bytes(4, bytes.clone()).unwrap();
        assert_eq!(again.count(), 3);
        assert_eq!(slots_of(&again, 3), vec![0, 2]);
        let edge = again.neighbors(0).next().unwrap();
        assert_eq!((edge.to, edge.kind), (3, LinkKind::Semantic));
        assert!((edge.w - 0.75).abs() < 1e-6);
        assert_eq!(again.to_bytes(4), bytes);
        // Wrong number of notes, a cut file, a link out of range, an unknown kind.
        assert!(Graph::from_bytes(3, bytes.clone()).is_none());
        assert!(Graph::from_bytes(4, bytes[..bytes.len() - 1].to_vec()).is_none());
        let mut bad = bytes.clone();
        let last = bad.len() - 9;
        bad[last] = 99;
        assert!(Graph::from_bytes(4, bad).is_none());
        let mut bad = bytes;
        let last = bad.len() - 1;
        bad[last] = 7;
        assert!(Graph::from_bytes(4, bad).is_none());
        assert!(Graph::from_bytes(0, Vec::new()).is_none());
        assert_eq!(Graph::from_bytes(0, Graph::default().to_bytes(0)).unwrap().count(), 0);
    }

    #[test]
    fn links_made_after_loading_and_removals_work_over_the_stored_ones() {
        let mut g = Graph::default();
        g.add(0, 1, LinkKind::Semantic, 0.7);
        g.add(1, 2, LinkKind::Temporal, 0.5);
        let mut g = Graph::from_bytes(3, g.to_bytes(3)).unwrap();
        g.ensure(5);
        g.add(1, 4, LinkKind::Causal, 1.0);
        g.add(3, 4, LinkKind::Temporal, 0.4);
        assert_eq!(slots_of(&g, 1), vec![0, 2, 4]);
        assert_eq!(g.count(), 4);
        g.remove_node(1);
        assert_eq!(g.count(), 1);
        assert_eq!(slots_of(&g, 0), Vec::<u32>::new());
        assert_eq!(slots_of(&g, 4), vec![3]);
        let bytes = g.to_bytes(5);
        let again = Graph::from_bytes(5, bytes).unwrap();
        assert_eq!(again.count(), 1);
        assert_eq!(slots_of(&again, 3), vec![4]);
    }

    #[test]
    fn temporal_neighbours_come_nearest_first_with_the_weight_formula() {
        let hour = 3600;
        let times: BTreeSet<(i64, u32)> =
            [(100 * hour, 0), (101 * hour, 1), (99 * hour, 2), (112 * hour, 3), (140 * hour, 4)].into_iter().collect();
        let found = temporal_neighbors(&times, 100 * hour + 1, 10);
        let slots: Vec<u32> = found.iter().map(|f| f.0).collect();
        assert_eq!(slots, vec![0, 1, 2, 3]);
        assert!((found[0].1 - 1.0).abs() < 0.01);
        assert!((found[3].1 - (1.0 - 12.0 / 24.0)).abs() < 0.01);
    }

    #[test]
    fn the_weight_never_falls_below_a_third_and_the_limit_holds() {
        let hour = 3600;
        let times: BTreeSet<(i64, u32)> = (0..30).map(|n| (n as i64 * hour / 2, n)).collect();
        let found = temporal_neighbors(&times, 14 * hour, 5);
        assert_eq!(found.len(), 5);
        let far = temporal_neighbors(&[(0i64, 0u32)].into_iter().collect(), 23 * hour + 1800, 5);
        assert_eq!(far.len(), 1);
        assert!((far[0].1 - 0.3).abs() < 1e-6);
        let beyond = temporal_neighbors(&[(0i64, 0u32)].into_iter().collect(), 25 * hour, 5);
        assert!(beyond.is_empty());
    }
}
