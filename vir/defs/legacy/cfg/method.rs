// © 2019, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::iter::FromIterator;
use uuid::Uuid;
use crate::legacy::ast::*;

pub const RETURN_LABEL: &str = "end_of_method";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfgMethod {
    // TODO: extract logic using (most) skipped fields to CfgMethodBuilder
    #[serde(skip)]
    pub(super) uuid: Uuid,
    pub(super) method_name: String,
    pub(in super::super) formal_arg_count: usize,
    pub(in super::super) formal_returns: Vec<LocalVar>,
    // FIXME: This should be pub(in super::super). However, the optimization
    // that depends on snapshots needs to modify this field.
    pub local_vars: Vec<LocalVar>,
    pub(super) labels: HashSet<String>,
    #[serde(skip)]
    pub(super) reserved_labels: HashSet<String>,
    pub basic_blocks: Vec<CfgBlock>, // FIXME: Hack, should be pub(super).
    pub(super) basic_blocks_labels: Vec<String>,
    #[serde(skip)]
    fresh_var_index: i32,
    #[serde(skip)]
    pub(crate) fresh_label_index: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfgBlock {
    pub stmts: Vec<Stmt>, // FIXME: Hack, should be pub(super).
    pub successor: Successor,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Successor {
    Undefined,
    Return,
    Goto(CfgBlockIndex),
    GotoSwitch(Vec<(Expr, CfgBlockIndex)>, CfgBlockIndex),
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize)]
pub struct CfgBlockIndex {
    #[serde(skip)]
    pub(crate) method_uuid: Uuid,
    pub block_index: usize,
}

impl fmt::Debug for CfgBlockIndex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "cfg:{}", self.block_index)
    }
}

impl Successor {
    pub fn is_return(&self) -> bool {
        matches!(self, Successor::Return)
    }

    pub fn get_following(&self) -> Vec<CfgBlockIndex> {
        match &self {
            Successor::Undefined | Successor::Return => vec![],
            Successor::Goto(target) => vec![*target],
            Successor::GotoSwitch(guarded_targets, default_target) => {
                let mut res: Vec<CfgBlockIndex> = guarded_targets.iter().map(|g| g.1).collect();
                res.push(*default_target);
                res
            }
        }
    }

    pub fn replace_target(self, src: CfgBlockIndex, dst: CfgBlockIndex) -> Self {
        assert_eq!(
            src.method_uuid, dst.method_uuid,
            "The provided src CfgBlockIndex is not compatible with the dst CfgBlockIndex"
        );
        match self {
            Successor::Goto(target) => Successor::Goto(if target == src { dst } else { target }),
            Successor::GotoSwitch(guarded_targets, default_target) => Successor::GotoSwitch(
                guarded_targets
                    .into_iter()
                    .map(|x| (x.0, if x.1 == src { dst } else { x.1 }))
                    .collect(),
                if default_target == src {
                    dst
                } else {
                    default_target
                },
            ),
            x => x,
        }
    }

    pub(super) fn replace_uuid(self, new_uuid: Uuid) -> Self {
        match self {
            Successor::Goto(target) => Successor::Goto(target.set_uuid(new_uuid)),
            Successor::GotoSwitch(guarded_targets, default_target) => Successor::GotoSwitch(
                guarded_targets
                    .into_iter()
                    .map(|x| (x.0, x.1.set_uuid(new_uuid)))
                    .collect(),
                default_target.set_uuid(new_uuid),
            ),
            x => x,
        }
    }
}

impl CfgBlockIndex {
    pub(super) fn set_uuid(self, method_uuid: Uuid) -> Self {
        CfgBlockIndex {
            method_uuid,
            ..self
        }
    }
    pub fn weak_eq(&self, other: &CfgBlockIndex) -> bool {
        self.block_index == other.block_index
    }
    pub fn index(&self) -> usize {
        self.block_index
    }
}

impl CfgMethod {
    pub fn new(
        method_name: String,
        formal_arg_count: usize,
        formal_returns: Vec<LocalVar>,
        local_vars: Vec<LocalVar>,
        reserved_labels: Vec<String>,
    ) -> Self {
        CfgMethod {
            uuid: Uuid::new_v4(),
            method_name,
            formal_arg_count,
            formal_returns,
            local_vars,
            labels: HashSet::new(),
            reserved_labels: HashSet::from_iter(reserved_labels),
            basic_blocks: vec![],
            basic_blocks_labels: vec![],
            fresh_var_index: 0,
            fresh_label_index: 0,
        }
    }

    pub fn name(&self) -> String {
        self.method_name.clone()
    }

    pub fn labels(&self) -> &HashSet<String> {
        &self.labels
    }

    pub fn basic_blocks_labels(&self) -> &Vec<String> {
        &self.basic_blocks_labels
    }

    pub fn get_formal_returns(&self) -> &Vec<LocalVar> {
        &self.formal_returns
    }

    pub(super) fn block_index(&self, index: usize) -> CfgBlockIndex {
        CfgBlockIndex {
            method_uuid: self.uuid,
            block_index: index,
        }
    }

    fn is_fresh_local_name(&self, name: &str) -> bool {
        self.formal_returns.iter().all(|x| x.name != name)
            && self.local_vars.iter().all(|x| x.name != name)
            && !self.labels.contains(name)
            && self.basic_blocks_labels.iter().all(|x| x != name)
    }

    fn generate_fresh_local_var_name(&mut self) -> String {
        let mut candidate_name = format!("__t{}", self.fresh_var_index);
        self.fresh_var_index += 1;
        while !self.is_fresh_local_name(&candidate_name)
            || self.reserved_labels.contains(&candidate_name)
        {
            candidate_name = format!("__t{}", self.fresh_var_index);
            self.fresh_var_index += 1;
        }
        candidate_name
    }

    pub fn get_fresh_label_name(&mut self) -> String {
        let mut candidate_name = format!("l{}", self.fresh_label_index);
        self.fresh_label_index += 1;
        while !self.is_fresh_local_name(&candidate_name)
            || self.reserved_labels.contains(&candidate_name)
        {
            candidate_name = format!("l{}", self.fresh_label_index);
            self.fresh_label_index += 1;
        }
        candidate_name
    }

    /// Returns all formal arguments, formal returns, and local variables
    pub fn get_all_vars(&self) -> Vec<LocalVar> {
        let mut vars: Vec<LocalVar> = vec![];
        vars.extend(self.formal_returns.clone());
        vars.extend(self.local_vars.clone());
        vars
    }

    /// Returns all labels
    pub fn get_all_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = vec![];
        labels.extend(self.labels.iter().cloned());
        labels.extend(self.basic_blocks_labels.iter().cloned());
        labels
    }

    pub fn add_fresh_local_var(&mut self, typ: Type) -> LocalVar {
        let name = self.generate_fresh_local_var_name();
        let local_var = LocalVar::new(name, typ);
        self.local_vars.push(local_var.clone());
        local_var
    }

    pub fn add_local_var(&mut self, name: &str, typ: Type) {
        assert!(self.is_fresh_local_name(name));
        self.local_vars.push(LocalVar::new(name, typ));
    }

    pub fn add_formal_return(&mut self, name: &str, typ: Type) {
        assert!(self.is_fresh_local_name(name));
        self.formal_returns.push(LocalVar::new(name, typ));
    }

    pub fn add_stmt(&mut self, index: CfgBlockIndex, stmt: Stmt) {
        for label_name in gather_labels(&stmt) {
            assert!(
                self.is_fresh_local_name(&label_name),
                "label {} is not fresh",
                label_name
            );
            self.labels.insert(label_name);
        }
        self.basic_blocks[index.block_index].stmts.push(stmt);
    }

    pub fn add_stmts(&mut self, index: CfgBlockIndex, stmts: Vec<Stmt>) {
        for stmt in stmts {
            self.add_stmt(index, stmt);
        }
    }

    pub fn add_block(&mut self, label: &str, stmts: Vec<Stmt>) -> CfgBlockIndex {
        assert!(label.chars().take(1).all(|c| c.is_alphabetic() || c == '_'));
        assert!(label
            .chars()
            .skip(1)
            .all(|c| c.is_alphanumeric() || c == '_'));
        assert!(
            self.basic_blocks_labels.iter().all(|l| l != label),
            "Label {} is already used",
            label
        );
        assert!(label != RETURN_LABEL);
        let index = self.basic_blocks.len();
        self.basic_blocks_labels.push(label.to_string());
        self.basic_blocks.push(CfgBlock {
            stmts,
            successor: Successor::Undefined,
        });
        self.block_index(index)
    }

    #[allow(dead_code)]
    pub fn get_successor(&mut self, index: CfgBlockIndex) -> &Successor {
        assert_eq!(
            self.uuid, index.method_uuid,
            "The provided CfgBlockIndex doesn't belong to this CfgMethod"
        );
        &self.basic_blocks[index.block_index].successor
    }

    #[allow(dead_code)]
    pub fn set_successor(&mut self, index: CfgBlockIndex, successor: Successor) {
        assert_eq!(
            self.uuid, index.method_uuid,
            "The provided CfgBlockIndex doesn't belong to this CfgMethod"
        );
        self.basic_blocks[index.block_index].successor = successor;
    }

    pub fn get_preceding(&self, target_index: CfgBlockIndex) -> Vec<CfgBlockIndex> {
        assert_eq!(
            self.uuid, target_index.method_uuid,
            "The provided CfgBlockIndex doesn't belong to this CfgMethod"
        );
        self.basic_blocks
            .iter()
            .enumerate()
            .filter(|x| x.1.successor.get_following().contains(&target_index))
            .map(|x| self.block_index(x.0))
            .collect()
    }

    #[allow(dead_code)]
    pub fn predecessors(&self) -> HashMap<usize, Vec<usize>> {
        let mut result = HashMap::new();
        for (index, block) in self.basic_blocks.iter().enumerate() {
            for successor in block.successor.get_following() {
                let entry = result
                    .entry(successor.block_index)
                    .or_insert_with(|| Vec::new());
                entry.push(index);
            }
        }
        result
    }

    #[allow(dead_code)]
    pub fn get_indices(&self) -> Vec<CfgBlockIndex> {
        (0..self.basic_blocks.len())
            .map(|i| self.block_index(i))
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_block_label(&self, index: CfgBlockIndex) -> &str {
        &self.basic_blocks_labels[index.block_index]
    }

    pub fn has_loops(&self) -> bool {
        let mut in_degree = vec![0; self.basic_blocks.len()];

        for index in 0..self.basic_blocks.len() {
            for succ in self.basic_blocks[index].successor.get_following() {
                in_degree[succ.index()] += 1;
            }
        }

        let mut queue = VecDeque::new();
        for index in 0..self.basic_blocks.len() {
            if in_degree[index] == 0 {
                queue.push_back(index);
            }
        }

        let mut visited_count = 0;

        while let Some(curr_index) = queue.pop_front() {
            for succ in self.basic_blocks[curr_index].successor.get_following() {
                in_degree[succ.index()] -= 1;

                if in_degree[succ.index()] == 0 {
                    queue.push_back(succ.index());
                }
            }
            visited_count += 1;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfgBlock {
    pub stmts: Vec<Stmt>, // FIXME: Hack, should be pub(super).
    pub(in super::super) successor: Successor,
}

impl CfgBlock {
    // FIXME: should not allow such constructor to be publicly accessible (currently for conversion)
    pub fn new(
        stmts: Vec<Stmt>,
        successor: Successor,
    ) -> Self {
        CfgBlock {
            stmts: stmts,
            successor: successor,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Successor {
    Undefined,
    Return,
    Goto(CfgBlockIndex),
    GotoSwitch(Vec<(Expr, CfgBlockIndex)>, CfgBlockIndex),
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize)]
pub struct CfgBlockIndex {
    #[serde(skip)]
    pub(super) method_uuid: Uuid,
    pub(in super::super) block_index: usize,
}

impl fmt::Debug for CfgBlockIndex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "cfg:{}", self.block_index)
    }
}

impl CfgBlockIndex {
    // FIXME: should not allow such constructor to be publicly accessible (currently for conversion)
    pub fn new(
        method_uuid: Uuid,
        block_index: usize,
    ) -> Self {
        CfgBlockIndex {
            method_uuid: method_uuid,
            block_index: block_index,
        }
    }
}