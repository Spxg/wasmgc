//! Arranges schema declarations for Wasm recursive type encoding.
//!
//! Array equivalence is refined through element references, while distinct
//! struct declarations retain their identity. Strongly connected components
//! become recursion groups, ordered after their external dependencies. Every
//! schema reference in operations, exports, and function signatures is remapped
//! whenever the schema sequence changes.

use std::{cmp::Ordering, collections::BTreeMap, ops::Range};

use super::{Input, Schema};
use crate::schema::{SchemaKind, ValueTag};

fn remap(input: &mut Input, indices: &[u32]) {
    for schema in &mut input.schemas {
        for field in &mut schema.fields {
            field.target = field.target.map(|target| indices[target as usize]);
        }
    }
    for operation in input.operations.values_mut() {
        operation.schema = indices[operation.schema as usize];
    }
    for export in &mut input.struct_exports {
        export.schema = indices[export.schema as usize];
    }
    for bridge in &mut input.bridges {
        for value in bridge.params.iter_mut().chain(&mut bridge.result) {
            value.target = value.target.map(|target| indices[target as usize]);
        }
    }
}

/// Merges equivalent array types and remaps all references to their schemas.
///
/// Rust may promote the same array descriptor at several addresses. Array
/// equivalence follows element references; struct declarations keep their identity.
pub(super) fn merge_array_types(input: &mut Input) {
    let schemas = &input.schemas;
    let arrays: Vec<_> = schemas
        .iter()
        .enumerate()
        .filter_map(|(index, schema)| (schema.kind == SchemaKind::Array).then_some(index))
        .collect();
    if arrays.len() < 2 {
        return;
    }

    let mut representatives = vec![0; schemas.len()];
    let mut next = representatives.clone();
    loop {
        let mut groups = BTreeMap::new();
        for &index in &arrays {
            let element = &schemas[index].fields[0];
            let target = element.target.map(|target| {
                let array = schemas[target as usize].kind == SchemaKind::Array;
                (
                    array,
                    if array {
                        representatives[target as usize]
                    } else {
                        target
                    },
                )
            });
            // The first member's schema index identifies the entire class.
            next[index] = *groups.entry((element.tag, target)).or_insert(index as u32);
        }
        if next == representatives {
            break;
        }
        core::mem::swap(&mut representatives, &mut next);
    }

    let mut indices = vec![0; schemas.len()];
    let mut merged = Vec::new();
    for (index, schema) in core::mem::take(&mut input.schemas).into_iter().enumerate() {
        if schema.kind == SchemaKind::Array && representatives[index] != index as u32 {
            indices[index] = indices[representatives[index] as usize];
            continue;
        }
        indices[index] = merged.len() as u32;
        merged.push(schema);
    }
    input.schemas = merged;
    remap(input, &indices);
}

/// Orders schema dependencies into canonical Wasm recursion groups.
pub(super) fn group_recursive_types(input: &mut Input) {
    let components = components(&input.schemas);
    let mut canonicalizer = Canonicalizer::new(&input.schemas, &components);
    let mut order = Vec::with_capacity(input.schemas.len());
    let mut groups: Vec<Range<usize>> = Vec::with_capacity(components.len());
    for members in components {
        let start = order.len();
        order.extend(canonicalizer.order(members));
        groups.push(start..order.len());
    }

    let mut indices = vec![0; input.schemas.len()];
    for (new, &old) in order.iter().enumerate() {
        indices[old] = new as u32;
    }
    input.schemas = order
        .into_iter()
        .map(|index| {
            let schema = &mut input.schemas[index];
            Schema {
                kind: schema.kind,
                fields: core::mem::take(&mut schema.fields),
            }
        })
        .collect();
    remap(input, &indices);
    input.schema_groups = groups;
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TypeKey {
    group: usize,
    member: usize,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SchemaKey {
    kind: SchemaKind,
    fields: Vec<(ValueTag, Option<TargetKey>)>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TargetKey {
    Internal(usize),
    External(TypeKey),
}

struct Canonicalizer<'a> {
    schemas: &'a [Schema],
    group_of: Vec<usize>,
    keys: Vec<Option<TypeKey>>,
    arena: Vec<Vec<SchemaKey>>,
    interned: BTreeMap<Vec<SchemaKey>, usize>,
    positions: Vec<usize>,
}

impl<'a> Canonicalizer<'a> {
    fn new(schemas: &'a [Schema], components: &[Vec<usize>]) -> Self {
        let mut group_of = vec![0; schemas.len()];
        for (group, members) in components.iter().enumerate() {
            for &index in members {
                group_of[index] = group;
            }
        }

        Self {
            schemas,
            group_of,
            keys: vec![None; schemas.len()],
            arena: Vec::new(),
            interned: BTreeMap::new(),
            positions: vec![usize::MAX; schemas.len()],
        }
    }

    fn order(&mut self, members: Vec<usize>) -> Vec<usize> {
        if members.len() == 1 {
            let index = members[0];
            self.positions[index] = 0;
            let key = self.key(&members);
            self.positions[index] = usize::MAX;
            return self.register(members, key);
        }

        let group = self.group_of[members[0]];
        let kind = members
            .iter()
            .map(|&index| self.schemas[index].kind)
            .min()
            .unwrap();
        let mut best: Option<(Vec<usize>, Vec<SchemaKey>)> = None;
        let mut stack = Vec::with_capacity(members.len());

        // The root kind is the first part of the key, so a larger kind cannot win.
        // Field order fixes each traversal; try every remaining root to choose
        // the least key. Structurally symmetric members keep the first traversal.
        for &root in members
            .iter()
            .filter(|&&index| self.schemas[index].kind == kind)
        {
            let mut order = Vec::with_capacity(members.len());
            stack.push(root);
            while let Some(index) = stack.pop() {
                if self.positions[index] != usize::MAX {
                    continue;
                }

                self.positions[index] = order.len();
                order.push(index);
                stack.extend(
                    self.schemas[index]
                        .fields
                        .iter()
                        .rev()
                        .filter_map(|field| field.target)
                        .map(|target| target as usize)
                        .filter(|&target| self.group_of[target] == group),
                );
            }

            let key = self.key(&order);
            for &index in &order {
                self.positions[index] = usize::MAX;
            }
            if best
                .as_ref()
                .is_none_or(|(_, best)| compare(&key, best, &self.arena).is_lt())
            {
                best = Some((order, key));
            }
        }

        let (order, key) = best.unwrap();
        self.register(order, key)
    }

    fn key(&self, order: &[usize]) -> Vec<SchemaKey> {
        let group = self.group_of[order[0]];
        order
            .iter()
            .map(|&index| SchemaKey {
                kind: self.schemas[index].kind,
                fields: self.schemas[index]
                    .fields
                    .iter()
                    .map(|field| {
                        let target = field.target.map(|target| {
                            let target = target as usize;
                            if self.group_of[target] == group {
                                TargetKey::Internal(self.positions[target])
                            } else {
                                TargetKey::External(self.keys[target].unwrap())
                            }
                        });
                        (field.tag, target)
                    })
                    .collect(),
            })
            .collect()
    }

    fn register(&mut self, order: Vec<usize>, key: Vec<SchemaKey>) -> Vec<usize> {
        // Keys reference earlier groups by index, avoiding recursive comparison
        // and destruction of owned keys along long dependency chains.
        let group = *self.interned.entry(key.clone()).or_insert_with(|| {
            let group = self.arena.len();
            self.arena.push(key);
            group
        });
        for (member, &index) in order.iter().enumerate() {
            self.keys[index] = Some(TypeKey { group, member });
        }
        order
    }
}

// Arena indices establish equality, but their allocation order is incidental.
// Compare referenced structures with an explicit stack so unrelated discovery
// order cannot choose a different recursion-group traversal.
fn compare(left: &[SchemaKey], right: &[SchemaKey], arena: &[Vec<SchemaKey>]) -> Ordering {
    enum Task<'a> {
        Schemas(&'a [SchemaKey], &'a [SchemaKey]),
        Fields(
            &'a [(ValueTag, Option<TargetKey>)],
            &'a [(ValueTag, Option<TargetKey>)],
        ),
        Order(Ordering),
    }

    let mut stack = vec![Task::Schemas(left, right)];
    while let Some(task) = stack.pop() {
        let order = match task {
            Task::Schemas(left, right) => {
                if let (Some((left, rest_left)), Some((right, rest_right))) =
                    (left.split_first(), right.split_first())
                {
                    stack.push(Task::Schemas(rest_left, rest_right));
                    stack.push(Task::Fields(&left.fields, &right.fields));
                    left.kind.cmp(&right.kind)
                } else {
                    left.len().cmp(&right.len())
                }
            }
            Task::Fields(left, right) => {
                if let (Some((left, rest_left)), Some((right, rest_right))) =
                    (left.split_first(), right.split_first())
                {
                    stack.push(Task::Fields(rest_left, rest_right));
                    let order = left.0.cmp(&right.0);
                    if order != Ordering::Equal {
                        return order;
                    }
                    match (&left.1, &right.1) {
                        (Some(TargetKey::External(left)), Some(TargetKey::External(right)))
                            if left.group != right.group =>
                        {
                            stack.push(Task::Order(left.member.cmp(&right.member)));
                            stack.push(Task::Schemas(&arena[left.group], &arena[right.group]));
                            Ordering::Equal
                        }
                        _ => left.1.cmp(&right.1),
                    }
                } else {
                    left.len().cmp(&right.len())
                }
            }
            Task::Order(order) => order,
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

fn components(schemas: &[Schema]) -> Vec<Vec<usize>> {
    let mut seen = vec![false; schemas.len()];
    let mut finished = Vec::new();

    for root in 0..schemas.len() {
        if seen[root] {
            continue;
        }

        let mut stack = vec![(root, false)];
        while let Some((index, finish)) = stack.pop() {
            if finish {
                finished.push(index);
                continue;
            }
            if core::mem::replace(&mut seen[index], true) {
                continue;
            }

            stack.push((index, true));
            stack.extend(
                schemas[index]
                    .fields
                    .iter()
                    .rev()
                    .filter_map(|field| field.target)
                    .map(|target| (target as usize, false)),
            );
        }
    }

    let mut incoming = vec![Vec::new(); schemas.len()];
    for (index, schema) in schemas.iter().enumerate() {
        for target in schema.fields.iter().filter_map(|field| field.target) {
            incoming[target as usize].push(index);
        }
    }

    seen.fill(false);
    let mut groups = Vec::new();
    for root in finished.into_iter().rev() {
        if seen[root] {
            continue;
        }

        let mut members = Vec::new();
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            if core::mem::replace(&mut seen[index], true) {
                continue;
            }

            members.push(index);
            stack.extend(&incoming[index]);
        }
        groups.push(members);
    }

    // Edges run from a declaration to its field dependencies. Reverse the SCC
    // traversal order so references outside a group always point to earlier types.
    groups.reverse();
    groups
}
