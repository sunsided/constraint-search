//! The variable–value graph of an all-different constraint: a maximum matching, and the
//! edges no maximum matching can use.
//!
//! An edge `(x, v)` is in some maximum matching exactly when it is in the matching found, lies
//! on an alternating cycle, or lies on an even alternating path that starts at a free value
//! (Berge). With matched edges pointing from variable to value and the others from value to
//! variable, the cycles are the strongly connected components and the paths are what a free
//! value reaches.
//!
//! All buffers live in [`MatchingScratch`] and keep their capacity between runs, so a run
//! allocates only while the buffers are still growing. Adjacency is stored flat (compressed
//! rows: `start[node]..start[node + 1]` indexes the neighbours).

use alloc::vec::Vec;

use crate::{Domain, Domains, Var};

/// Marks an unmatched variable or value, and an unvisited node.
const NONE: usize = usize::MAX;

/// Working memory of [`AllDifferent`](crate::AllDifferent), reused across propagation calls.
///
/// Graph nodes `0..n` are the variables, nodes `n..n + m` the values.
#[derive(Debug, Default)]
pub struct MatchingScratch {
    /// The values of the union of the domains, sorted ascending.
    values: Vec<u32>,
    /// `edges[edge_start[x]..edge_start[x + 1]]` are the value indices in variable `x`'s
    /// domain. One longer than the number of variables.
    edge_start: Vec<usize>,
    /// The value indices of all variables' domains, back to back.
    edges: Vec<usize>,
    /// For each variable, the value index it is matched to, or [`NONE`].
    var_match: Vec<usize>,
    /// For each value index, the variable it is matched to, or [`NONE`].
    value_match: Vec<usize>,
    /// Values visited by the current augmenting-path search.
    seen: Vec<bool>,
    /// `successors[successor_start[u]..successor_start[u + 1]]` are the nodes `u` points to.
    successor_start: Vec<usize>,
    /// The directed edges' targets, grouped by source node.
    successors: Vec<usize>,
    /// The next free slot per source node while filling [`MatchingScratch::successors`].
    cursor: Vec<usize>,
    /// Nodes reachable from a free value.
    reached: Vec<bool>,
    /// The depth-first stack of the reachability search.
    stack: Vec<usize>,
    /// Strongly connected components.
    tarjan: Tarjan,
    /// `(variable position, value)` pairs that no maximum matching uses.
    unsupported: Vec<(usize, u32)>,
}

impl MatchingScratch {
    /// Builds the graph of `vars` over their current domains.
    pub(super) fn build<D: Domain>(&mut self, domains: &Domains<D>, vars: &[Var]) {
        let union = vars
            .iter()
            .fold(D::empty(), |union, &var| union.union(domains[var]));
        self.values.clear();
        self.values.extend(union.values());

        self.edge_start.clear();
        self.edges.clear();
        for &var in vars {
            self.edge_start.push(self.edges.len());
            for value in domains[var].values() {
                let index = self.values.binary_search(&value);
                self.edges.push(index.expect("value is in the union"));
            }
        }

        self.edge_start.push(self.edges.len());
        reset(&mut self.var_match, vars.len(), NONE);
        reset(&mut self.value_match, self.values.len(), NONE);
    }

    /// The number of variables in the graph.
    fn vars(&self) -> usize {
        self.edge_start.len() - 1
    }

    /// The value indices in the domain of variable `var`.
    fn edges_of(&self, var: usize) -> core::ops::Range<usize> {
        self.edge_start[var]..self.edge_start[var + 1]
    }

    /// Matches every variable to a distinct value; returns `false` if that is impossible.
    pub(super) fn match_all(&mut self) -> bool {
        if self.values.len() < self.vars() {
            return false;
        }

        reset(&mut self.seen, self.values.len(), false);
        (0..self.vars()).all(|var| {
            self.seen.fill(false);
            self.augment(var)
        })
    }

    /// Finds an augmenting path from the unmatched `var`, skipping the values already seen.
    ///
    /// Recursion depth is at most the number of variables.
    fn augment(&mut self, var: usize) -> bool {
        for position in self.edges_of(var) {
            let value = self.edges[position];
            if self.seen[value] {
                continue;
            }

            self.seen[value] = true;
            let owner = self.value_match[value];
            if owner == NONE || self.augment(owner) {
                self.var_match[var] = value;
                self.value_match[value] = var;
                return true;
            }
        }

        false
    }

    /// The `(variable position, value)` pairs that no maximum matching uses, after
    /// [`MatchingScratch::match_all`] succeeded.
    pub(super) fn unsupported(&mut self) -> &[(usize, u32)] {
        self.build_successors();
        self.reach_from_free();
        self.tarjan.run(&self.successor_start, &self.successors);

        let vars = self.vars();
        self.unsupported.clear();
        for var in 0..vars {
            for position in self.edges_of(var) {
                let value = self.edges[position];
                let node = vars + value;
                let component = &self.tarjan.component;
                let used = self.var_match[var] == value
                    || component[var] == component[node]
                    || self.reached[node];
                if !used {
                    self.unsupported.push((var, self.values[value]));
                }
            }
        }

        &self.unsupported
    }

    /// The directed graph: matched edges from variable to value, the others reversed.
    fn build_successors(&mut self) {
        let vars = self.vars();
        let nodes = vars + self.values.len();
        reset(&mut self.successor_start, nodes + 1, 0);
        for var in 0..vars {
            for position in self.edges_of(var) {
                let (from, _) = self.direct(var, self.edges[position]);
                self.successor_start[from + 1] += 1;
            }
        }

        for node in 1..=nodes {
            self.successor_start[node] += self.successor_start[node - 1];
        }

        reset(&mut self.successors, self.successor_start[nodes], 0);
        self.cursor.clear();
        self.cursor
            .extend_from_slice(&self.successor_start[..nodes]);
        for var in 0..vars {
            for position in self.edges_of(var) {
                let (from, to) = self.direct(var, self.edges[position]);
                self.successors[self.cursor[from]] = to;
                self.cursor[from] += 1;
            }
        }
    }

    /// The direction of the edge between `var` and value index `value`, as `(from, to)` nodes.
    fn direct(&self, var: usize, value: usize) -> (usize, usize) {
        let node = self.vars() + value;
        if self.var_match[var] == value {
            (var, node)
        } else {
            (node, var)
        }
    }

    /// Marks every node reachable from a value that no variable is matched to.
    fn reach_from_free(&mut self) {
        let vars = self.vars();
        reset(&mut self.reached, self.successor_start.len() - 1, false);
        self.stack.clear();
        for value in 0..self.values.len() {
            if self.value_match[value] == NONE {
                self.stack.push(vars + value);
            }
        }

        while let Some(node) = self.stack.pop() {
            if self.reached[node] {
                continue;
            }

            self.reached[node] = true;
            let next = &self.successors[self.successor_start[node]..self.successor_start[node + 1]];
            self.stack
                .extend(next.iter().filter(|&&next| !self.reached[next]));
        }
    }
}

/// Empties `buffer` and refills it with `len` copies of `value`, keeping its capacity.
fn reset<T: Copy>(buffer: &mut Vec<T>, len: usize, value: T) {
    buffer.clear();
    buffer.resize(len, value);
}

/// The working state of Tarjan's strongly connected components algorithm, iterative.
#[derive(Debug, Default)]
struct Tarjan {
    /// The visiting order of each node; [`NONE`] while unvisited.
    index: Vec<usize>,
    /// The smallest visiting order reachable from each node through its subtree.
    low: Vec<usize>,
    /// `true` while the node is on [`Tarjan::stack`].
    on_stack: Vec<bool>,
    /// Visited nodes not yet assigned a component.
    stack: Vec<usize>,
    /// The simulated call stack: a node and the position of its next successor.
    calls: Vec<(usize, usize)>,
    /// The component of each node.
    component: Vec<usize>,
    /// The visiting order the next node gets.
    next_index: usize,
    /// The number the next component gets.
    next_component: usize,
}

impl Tarjan {
    /// Computes the component of every node of the graph given in compressed rows.
    fn run(&mut self, start: &[usize], successors: &[usize]) {
        let nodes = start.len() - 1;
        reset(&mut self.index, nodes, NONE);
        reset(&mut self.low, nodes, 0);
        reset(&mut self.on_stack, nodes, false);
        reset(&mut self.component, nodes, NONE);
        self.stack.clear();
        self.next_index = 0;
        self.next_component = 0;

        for root in 0..nodes {
            if self.index[root] == NONE {
                self.visit_from(root, start, successors);
            }
        }
    }

    /// Visits every unvisited node reachable from `root`, depth first without recursion.
    fn visit_from(&mut self, root: usize, start: &[usize], successors: &[usize]) {
        self.calls.clear();
        self.calls.push((root, start[root]));
        self.enter(root);

        while let Some(&(node, position)) = self.calls.last() {
            if position < start[node + 1] {
                let next = successors[position];
                let top = self.calls.len() - 1;
                self.calls[top].1 += 1;
                if self.index[next] == NONE {
                    self.enter(next);
                    self.calls.push((next, start[next]));
                } else if self.on_stack[next] {
                    self.low[node] = self.low[node].min(self.index[next]);
                }

                continue;
            }

            self.calls.pop();
            if let Some(&(parent, _)) = self.calls.last() {
                self.low[parent] = self.low[parent].min(self.low[node]);
            }

            if self.low[node] == self.index[node] {
                self.close_component(node);
            }
        }
    }

    /// Gives `node` its visiting order and puts it on the stack.
    fn enter(&mut self, node: usize) {
        self.index[node] = self.next_index;
        self.low[node] = self.next_index;
        self.next_index += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
    }

    /// Pops the component whose root is `root` off the stack.
    fn close_component(&mut self, root: usize) {
        while let Some(node) = self.stack.pop() {
            self.on_stack[node] = false;
            self.component[node] = self.next_component;
            if node == root {
                break;
            }
        }

        self.next_component += 1;
    }
}
