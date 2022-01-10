use crate::graph::*;
use crate::trio::*;
use crate::graph_algos::*;
use log::debug;
use std::collections::HashSet;
use std::collections::HashMap;

pub struct HaploPath {
    v_storage: Vec<Vertex>,
    l_storage: Vec<Link>,
    initial_node: usize,
}

//FIXME rename, doesn't know about haplotype!
//Never empty! Use None instead
impl HaploPath {

    fn new(init_v: Vertex) -> HaploPath {
        HaploPath {
            v_storage: vec![init_v],
            l_storage: Vec::new(),
            initial_node: init_v.node_id,
        }
    }

    pub fn vertices(&self) -> &Vec<Vertex> {
        &self.v_storage
    }

    pub fn start(&self) -> Vertex {
        self.v_storage[0]
    }

    pub fn end(&self) -> Vertex {
        self.v_storage[self.v_storage.len() - 1]
    }

    pub fn len(&self) -> usize {
        self.v_storage.len()
    }

    pub fn links(&self) -> &Vec<Link> {
        &self.l_storage
    }

    fn reverse_complement(self) -> HaploPath {
        //TODO optimize since consuming self
        HaploPath {
            v_storage: self.v_storage.iter().rev().map(|v| v.rc()).collect(),
            l_storage: self.l_storage.iter().rev().map(|l| l.rc()).collect(),
            initial_node: self.initial_node,
        }
    }

    fn trim_to(&mut self, v: &Vertex) -> bool {
        if self.v_storage.contains(v) {
            while self.v_storage.last().unwrap() != v {
                self.v_storage.pop();
                self.l_storage.pop();
            }
            return true;
        }
        false
    }

    fn append(&mut self, l: Link) {
        assert!(self.v_storage.last().unwrap() == &l.start);
        //TODO disable expensive assert?
        assert!(!self.in_path(l.end.node_id));
        self.v_storage.push(l.end);
        self.l_storage.push(l);
    }

    fn in_path(&self, node_id: usize) -> bool {
        self.v_storage.iter().any(|v| v.node_id == node_id)
    }

    fn can_merge_in(&self, path: &HaploPath) -> bool {
        assert!(self.v_storage.last() == path.v_storage.first());
        !path.v_storage.iter().skip(1).any(|v| self.in_path(v.node_id))
    }

    fn merge_in(&mut self, path: HaploPath) {
        assert!(self.can_merge_in(&path));
        for l in path.l_storage {
            self.append(l);
        }
    }

    pub fn initial_node(&self) -> usize {
        self.initial_node
    }

    pub fn print(&self, g: &Graph) -> String {
        self.v_storage.iter().map(|&v| g.v_str(v)).collect::<Vec<String>>().join(",")
    }

    pub fn print_gaf(&self, g: &Graph) -> String {
        self.v_storage.iter().map(|&v| g.gaf_str(v)).collect::<Vec<String>>().join("")
    }

}

//TODO add template parameter
pub struct HaploPathSearcher<'a> {
    g: &'a Graph,
    assignments: &'a AssignmentStorage<'a>,
    long_node_threshold: usize,
    //TODO consider using same structure as for initial assignments
    used: HashMap<usize, TrioGroup>,
    in_sccs: HashSet<usize>,
}

impl <'a> HaploPathSearcher<'a> {
    fn nodes_in_sccs(g: &Graph) -> HashSet<usize> {
        let mut nodes_in_sccs = HashSet::new();
        for scc in scc::strongly_connected(g) {
            for v in scc {
                nodes_in_sccs.insert(v.node_id);
            }
        }
        nodes_in_sccs
    }

    pub fn new(g: &'a Graph, assignments: &'a AssignmentStorage<'a>, long_node_threshold: usize) -> HaploPathSearcher<'a> {
        HaploPathSearcher{
            g,
            assignments,
            long_node_threshold,
            used: HashMap::new(),
            in_sccs: HaploPathSearcher::nodes_in_sccs(g),
        }
    }

    fn update_used(&mut self, path: &HaploPath, group: TrioGroup) {
        for v in path.vertices() {
            let blended = match self.used.get(&v.node_id) {
                Some(exist_group) => TrioGroup::blend(*exist_group, group),
                None => group,
            };
            self.used.insert(v.node_id, blended);
        }
    }

    pub fn used(&self) -> &HashMap<usize, TrioGroup> {
        &self.used
    }

    //TODO maybe use single length threshold?
    pub fn find_all(&mut self) -> Vec<(HaploPath, TrioGroup)> {
        let mut answer = Vec::new();

        for (node_id, node) in self.g.all_nodes().enumerate() {
            if self.used.contains_key(&node_id) {
                continue;
            }
            if node.length >= self.long_node_threshold && self.assignments.is_definite(node_id) {
                let group = self.assignments.get(node_id).unwrap().group;
                let path = self.haplo_path(node_id, group);
                self.update_used(&path, group);
                answer.push((path, group));
            }
        }
        answer
    }

    fn haplo_path(&self, node_id: usize, group: TrioGroup) -> HaploPath {
        assert!(!self.incompatible_assignment(node_id, group));
        let mut path = HaploPath::new(Vertex::forward(node_id));
        self.grow_jump_forward(&mut path, group);
        path = path.reverse_complement();
        self.grow_jump_forward(&mut path, group);
        path.reverse_complement()
    }

    //TODO maybe consume when grow?
    fn grow_jump_forward(&self, path: &mut HaploPath, group: TrioGroup) -> usize {
        let mut tot_grow = 0;
        loop {
            let mut grow = self.grow_forward(path, group, true);
            grow += self.jump_forward(path, group);
            if grow == 0 {
                break;
            }
            tot_grow += grow;
        }
        tot_grow
    }

    fn jump_forward(&self, path: &mut HaploPath, group: TrioGroup) -> usize {
        if let Some(jump) = self.find_jump_ahead(path.end(), group) {
            assert!(jump.len() > 1);
            assert!(path.end() == jump.start());
            //FIXME improve logging!
            if path.can_merge_in(&jump)
                //written this way only to skip last node, rewrite!
                && jump.l_storage.iter().all(|l| !self.in_sccs.contains(&l.start.node_id))
                && jump.v_storage.iter().all(|v| self.check_available(v.node_id, group)) {
                let add_on = jump.len() - 1;
                path.merge_in(jump);
                return add_on;
            }
        }
        0
    }

    fn inner_dfs(&self, v: Vertex, visited: &mut HashSet<Vertex>, long_ext: &mut Vec<Vertex>) {
        visited.insert(v);
        //if only one vertex is visited then it means we just started
        if visited.len() > 1 && self.g.node(v.node_id).length >= self.long_node_threshold {
            long_ext.push(v);
        } else {
            for l in self.g.outgoing_edges(v) {
                let w = l.end;
                if !visited.contains(&w) {
                    self.inner_dfs(w, visited, long_ext);
                }
            }
        }
    }

    fn bounded_dfs(&self, v: Vertex) -> Vec<Vertex> {
        //TODO change for integer vectors
        let mut visited = HashSet::new();
        let mut long_ext = Vec::new();
        self.inner_dfs(v, &mut visited, &mut long_ext);
        long_ext
    }

    fn try_link(&self, mut path: HaploPath, v: Vertex) -> HaploPath {
        for l in self.g.outgoing_edges(path.end()) {
            if l.end == v {
                path.append(l);
                break;
            }
        }
        path
    }

    fn long_node(&self, node_id: usize) -> bool {
        self.g.node(node_id).length >= self.long_node_threshold
    }

    fn link_vertex_check(&self, w: Vertex, group: TrioGroup) -> bool {
        let long_node_ahead = |v: Vertex| {
            assert!(self.g.outgoing_edge_cnt(v) == 1);
            self.long_node(self.g.outgoing_edges(v)[0].end.node_id)
        };

        !self.long_node(w.node_id)
            && !self.incompatible_assignment(w.node_id, group)
            && self.g.incoming_edge_cnt(w) == 1
            && self.g.outgoing_edge_cnt(w) == 1
            && (long_node_ahead(w)
                || long_node_ahead(w.rc())
                || self.check_assignment(w.node_id, group))
    }

    fn try_link_with_vertex(&self, mut path: HaploPath, v: Vertex, group: TrioGroup) -> HaploPath {
        let mut outgoing_edges = self.g.outgoing_edges(path.end());
        outgoing_edges.sort_by(|a, b| self.g.node(b.end.node_id).coverage
                        .partial_cmp(&self.g.node(a.end.node_id).coverage)
                        .unwrap());

        for l in outgoing_edges {
            let w = l.end;
            //TODO think if checks are reasonable //FIXME think if we should check coverage too
            if !path.in_path(w.node_id) && self.link_vertex_check(w, group) {
                if let Some(l2) = self.g.connector(w, v) {
                    debug!("Was able to link {} via {}", self.g.v_str(v), self.g.v_str(w));
                    path.append(l);
                    path.append(l2);
                    break;
                }
            }
        }
        path
    }

    fn find_jump_ahead(&self, v: Vertex, group: TrioGroup) -> Option<HaploPath> {
        debug!("Trying to jump ahead from {}", self.g.v_str(v));
        //Currently behavior is quite conservative:
        //1. all long nodes ahead should have assignment
        //2. only one should have correct assignment
        //3. this one should have unambiguous path backward to the vertex maybe stopping one link away
        let long_ahead: Vec<Vertex> = self.bounded_dfs(v);

        //println!("Long ahead: {}", long_ahead.iter().map(|x| self.g.v_str(*x)).collect::<Vec<String>>().join(";"));

        if long_ahead.iter().all(|x| self.assignments.is_definite(x.node_id)) {
            let potential_ext: Vec<Vertex> = long_ahead.into_iter()
                .filter(|x| self.assignments.get(x.node_id).unwrap().group == group)
                .collect();
            debug!("Assignment matching extension count: {}", potential_ext.len());
            if potential_ext.len() == 1 {
                debug!("Unique potential extension {}", self.g.v_str(potential_ext[0]));
                let mut p = HaploPath::new(potential_ext[0].rc());
                debug!("Growing path forward from {}", self.g.v_str(potential_ext[0]));
                self.grow_forward(&mut p, group, false);
                debug!("Found path {}", p.print(self.g));
                if !p.in_path(v.node_id) {
                    debug!("Tried linking via vertex");
                    p = self.try_link_with_vertex(p, v.rc(), group);
                }
                if !p.in_path(v.node_id) {
                    debug!("Tried linking");
                    p = self.try_link(p, v.rc());
                }
                if p.trim_to(&v.rc()) {
                    assert!(p.len() > 1);
                    let p = p.reverse_complement();
                    debug!("Successfully found jump, path {}", p.print(self.g));
                    return Some(p);
                }
                debug!("Couldn't trim to vertex {}", self.g.v_str(v.rc()));
            }
        } else {
            debug!("Not all long extensions had definite assignments");
        }

        debug!("Can't find jump");

        None
    }

    //FIXME maybe stop grow process immediately when this fails
    fn check_available(&self, node_id: usize, target_group: TrioGroup) -> bool {
        if let Some(&group) = self.used.get(&node_id) {
            assert!(group != TrioGroup::ISSUE);
            if TrioGroup::incompatible(group, target_group) {
                if self.long_node(node_id) {
                    debug!("Can't reuse long node {} in different haplotype", self.g.name(node_id));
                    return false;
                }
            } else {
                //node already used within the same haplotype
                debug!("Tried to reuse node {} twice within the same haplotype: {:?}",
                        self.g.name(node_id), target_group);
                return false;
            }
        }
        true
    }

    fn grow_forward(&self, path: &mut HaploPath, group: TrioGroup, check_avail: bool) -> usize {
        let mut v = path.end();
        let mut steps = 0;
        while let Some(l) = self.group_extension(v, group) {
            let w = l.end;
            if path.in_path(w.node_id)
                || (check_avail && !self.check_available(w.node_id, group)) {
                break;
            } else {
                path.append(l);
                v = w;
                steps += 1;
            }
        }
        steps
    }

    fn incompatible_assignment(&self, node_id: usize, target_group: TrioGroup) -> bool {
        if let Some(assign) = self.assignments.get(node_id) {
            if TrioGroup::incompatible(assign.group, target_group) {
                return true;
            }
        }
        false
    }

    fn check_assignment(&self, node_id: usize, target_group: TrioGroup) -> bool {
        if let Some(assign) = self.assignments.get(node_id) {
            if assign.group == target_group {
                return true;
            }
        }
        false
    }

    //maybe move to graph or some GraphAlgoHelper?
    fn unambiguous_extension(&self, v: Vertex) -> Option<Link> {
        //TODO simplify?
        match self.g.outgoing_edge_cnt(v) {
            1 => Some(self.g.outgoing_edges(v)[0]),
            _ => None,
        }
    }

    //maybe move to graph or some GraphAlgoHelper?
    fn group_extension(&self, v: Vertex, group: TrioGroup) -> Option<Link> {
        if let Some(l) = self.unambiguous_extension(v) {
            if !self.incompatible_assignment(l.end.node_id, group) {
                return Some(l);
            }
        }

        let mut suitable_extension = None;
        for l in self.g.outgoing_edges(v) {
            let w = l.end;
            if self.assignments.is_definite(w.node_id) {
                //FIXME helper method
                if self.assignments.get(w.node_id).unwrap().group == group {
                    if suitable_extension.is_none() {
                        suitable_extension = Some(l);
                    } else {
                        return None;
                    }
                }
            } else {
                return None;
            }
        }
        suitable_extension
    }

}