//! The variable–value graph of an all-different constraint: a maximum matching, and the
//! edges no maximum matching can use.
//!
//! An edge `(x, v)` is in some maximum matching exactly when it is in the matching found, lies
//! on an alternating cycle, or lies on an even alternating path that starts at a free value
//! (Berge). With matched edges pointing from variable to value and the others from value to
//! variable, the cycles are the strongly connected components and the paths are what a free
//! value reaches.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Domain, Domains, Var};

/// The bipartite graph between the variables of one constraint and the values they may take.
///
/// Graph nodes `0..n` are the variables, nodes `n..n + m` the values.
pub(super) struct ValueGraph {
    /// The values of the union of the domains, sorted ascending.
    values: Vec<u32>,
    /// For each variable, the indices into [`ValueGraph::values`] of its domain.
    edges: Vec<Vec<usize>>,
    /// For each variable, the value index it is matched to.
    var_match: Vec<Option<usize>>,
    /// For each value index, the variable it is matched to.
    value_match: Vec<Option<usize>>,
}

impl ValueGraph {
    /// The graph of `vars` over their current domains.
    pub(super) fn new<D: Domain>(domains: &Domains<D>, vars: &[Var]) -> Self {
        let union = vars
            .iter()
            .fold(D::empty(), |union, &var| union.union(domains[var]));
        let values: Vec<u32> = union.values().collect();
        let edges = vars
            .iter()
            .map(|&var| {
                domains[var]
                    .values()
                    .map(|value| values.binary_search(&value).expect("value is in the union"))
                    .collect()
            })
            .collect();

        Self {
            var_match: vec![None; vars.len()],
            value_match: vec![None; values.len()],
            values,
            edges,
        }
    }

    /// Matches every variable to a distinct value; returns `false` if that is impossible.
    pub(super) fn match_all(&mut self) -> bool {
        if self.values.len() < self.edges.len() {
            return false;
        }

        let mut seen = vec![false; self.values.len()];
        (0..self.edges.len()).all(|var| {
            seen.fill(false);
            self.augment(var, &mut seen)
        })
    }

    /// Finds an augmenting path from the unmatched `var`, skipping the values in `seen`.
    ///
    /// Recursion depth is at most the number of variables.
    fn augment(&mut self, var: usize, seen: &mut [bool]) -> bool {
        for position in 0..self.edges[var].len() {
            let value = self.edges[var][position];
            if seen[value] {
                continue;
            }

            seen[value] = true;
            let owner = self.value_match[value];
            if owner.is_none_or(|owner| self.augment(owner, seen)) {
                self.var_match[var] = Some(value);
                self.value_match[value] = Some(var);
                return true;
            }
        }

        false
    }

    /// The `(variable position, value)` pairs that no maximum matching uses, after
    /// [`ValueGraph::match_all`] succeeded.
    pub(super) fn unsupported(&self) -> Vec<(usize, u32)> {
        let successors = self.successors();
        let component = components(&successors);
        let reached = self.reached_from_free(&successors);
        let vars = self.edges.len();

        let mut unsupported = Vec::new();
        for (var, values) in self.edges.iter().enumerate() {
            for &value in values {
                let node = vars + value;
                let used = self.var_match[var] == Some(value)
                    || component[var] == component[node]
                    || reached[node];
                if !used {
                    unsupported.push((var, self.values[value]));
                }
            }
        }

        unsupported
    }

    /// The directed graph: matched edges from variable to value, the others reversed.
    fn successors(&self) -> Vec<Vec<usize>> {
        let vars = self.edges.len();
        let mut successors = vec![Vec::new(); vars + self.values.len()];
        for (var, values) in self.edges.iter().enumerate() {
            for &value in values {
                if self.var_match[var] == Some(value) {
                    successors[var].push(vars + value);
                } else {
                    successors[vars + value].push(var);
                }
            }
        }

        successors
    }

    /// Marks every node reachable from a value that no variable is matched to.
    fn reached_from_free(&self, successors: &[Vec<usize>]) -> Vec<bool> {
        let vars = self.edges.len();
        let mut reached = vec![false; successors.len()];
        let mut stack: Vec<usize> = (0..self.values.len())
            .filter(|&value| self.value_match[value].is_none())
            .map(|value| vars + value)
            .collect();

        while let Some(node) = stack.pop() {
            if !reached[node] {
                reached[node] = true;
                stack.extend(successors[node].iter().filter(|&&next| !reached[next]));
            }
        }

        reached
    }
}

/// The strongly connected component of each node, by an iterative Tarjan's algorithm.
fn components(successors: &[Vec<usize>]) -> Vec<usize> {
    let mut tarjan = Tarjan {
        index: vec![usize::MAX; successors.len()],
        low: vec![0; successors.len()],
        on_stack: vec![false; successors.len()],
        stack: Vec::new(),
        component: vec![usize::MAX; successors.len()],
        next_index: 0,
        next_component: 0,
    };

    for root in 0..successors.len() {
        if tarjan.index[root] == usize::MAX {
            tarjan.visit_from(root, successors);
        }
    }

    tarjan.component
}

/// The working state of Tarjan's strongly connected components algorithm.
struct Tarjan {
    /// The visiting order of each node; `usize::MAX` while unvisited.
    index: Vec<usize>,
    /// The smallest visiting order reachable from each node through its subtree.
    low: Vec<usize>,
    /// `true` while the node is on [`Tarjan::stack`].
    on_stack: Vec<bool>,
    /// Visited nodes not yet assigned a component.
    stack: Vec<usize>,
    /// The component of each node; `usize::MAX` until assigned.
    component: Vec<usize>,
    /// The visiting order the next node gets.
    next_index: usize,
    /// The number the next component gets.
    next_component: usize,
}

impl Tarjan {
    /// Visits every unvisited node reachable from `root`, depth first without recursion.
    fn visit_from(&mut self, root: usize, successors: &[Vec<usize>]) {
        let mut calls = vec![(root, 0)];
        self.enter(root);

        while let Some(&mut (node, ref mut position)) = calls.last_mut() {
            if let Some(&next) = successors[node].get(*position) {
                *position += 1;
                if self.index[next] == usize::MAX {
                    self.enter(next);
                    calls.push((next, 0));
                } else if self.on_stack[next] {
                    self.low[node] = self.low[node].min(self.index[next]);
                }

                continue;
            }

            calls.pop();
            if let Some(&(parent, _)) = calls.last() {
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
