use super::{Resolver, Schema, Transformer, Value};
use crate::schema::{SchemaKind, ValueTag};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
};
use wasm_encoder::{
    ArrayType, CompositeInnerType, CompositeType, FieldType, HeapType, RefType, StorageType,
    StructType, SubType, TypeSection, ValType,
};

/// Orders recursion groups after their dependencies and each group's members by structure.
pub fn groups(schemas: &HashMap<u64, Schema>) -> Vec<Vec<u64>> {
    let mut addresses: Vec<_> = schemas.keys().copied().collect();
    addresses.sort_unstable();

    let mut seen = HashSet::with_capacity(schemas.len());
    let mut finished = Vec::with_capacity(schemas.len());

    for &root in &addresses {
        if seen.contains(&root) {
            continue;
        }

        let mut stack = vec![(root, false)];
        while let Some((address, finish)) = stack.pop() {
            if finish {
                finished.push(address);
                continue;
            }
            if !seen.insert(address) {
                continue;
            }

            stack.push((address, true));
            stack.extend(
                schemas[&address]
                    .fields
                    .iter()
                    .rev()
                    .filter_map(|value| value.target_addr)
                    .map(|target| (target, false)),
            );
        }
    }

    let mut incoming: HashMap<_, Vec<_>> = addresses
        .iter()
        .copied()
        .map(|address| (address, Vec::new()))
        .collect();
    for &address in &addresses {
        for target in schemas[&address]
            .fields
            .iter()
            .filter_map(|value| value.target_addr)
        {
            incoming.get_mut(&target).unwrap().push(address);
        }
    }

    seen.clear();
    let mut groups = Vec::new();
    for root in finished.into_iter().rev() {
        if seen.contains(&root) {
            continue;
        }

        let mut members = Vec::new();
        let mut stack = vec![root];
        while let Some(address) = stack.pop() {
            if !seen.insert(address) {
                continue;
            }

            members.push(address);
            stack.extend(incoming[&address].iter().copied());
        }
        members.sort_unstable();
        groups.push(members);
    }

    // Edges point from each type to its dependencies, so reverse the SCC order.
    groups.reverse();
    order_groups(schemas, &mut groups);
    groups
}

impl Resolver<'_> {
    pub fn resolve_types(&mut self) {
        self.merge_array_types();
        self.type_groups = groups(&self.schema_set);
        self.type_indices = self
            .type_groups
            .iter()
            .flatten()
            .enumerate()
            .map(|(offset, &address)| (address, self.original_types + offset as u32))
            .collect();
    }

    fn merge_array_types(&mut self) {
        let mut arrays: Vec<_> = self
            .schema_set
            .iter()
            .filter_map(|(&address, schema)| (schema.kind == SchemaKind::Array).then_some(address))
            .collect();
        if arrays.len() < 2 {
            return;
        }
        arrays.sort_unstable();

        // Begin with one array class, then distinguish its element dependencies.
        // Struct targets retain their declaration identity throughout refinement.
        let mut representatives: HashMap<_, _> =
            arrays.iter().map(|&address| (address, 0)).collect();
        let mut next = representatives.clone();
        loop {
            let mut classes = BTreeMap::new();
            for &address in &arrays {
                let element = &self.schema_set[&address].fields[0];
                let target = element.target_addr.map(|target| {
                    (
                        self.schema_set[&target].kind,
                        representatives.get(&target).copied().unwrap_or(target),
                    )
                });
                next.insert(
                    address,
                    *classes.entry((element.tag, target)).or_insert(address),
                );
            }
            if next == representatives {
                break;
            }
            core::mem::swap(&mut representatives, &mut next);
        }

        let remap = |address: u64| representatives.get(&address).copied().unwrap_or(address);
        self.schema_set
            .retain(|&address, _| remap(address) == address);

        let values = self
            .schema_set
            .values_mut()
            .flat_map(|schema| &mut schema.fields)
            .chain(
                self.function_imports
                    .iter_mut()
                    .flat_map(|import| import.params.iter_mut().chain(&mut import.result)),
            )
            .chain(
                self.function_exports
                    .iter_mut()
                    .flat_map(|export| export.params.iter_mut().chain(&mut export.result)),
            );
        for value in values {
            value.target_addr = value.target_addr.map(remap);
        }
        for operation in self.operations.values_mut() {
            operation.schema_addr = remap(operation.schema_addr);
        }
        for export in &mut self.struct_exports {
            export.schema_addr = remap(export.schema_addr);
        }
    }
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

fn order_groups(schemas: &HashMap<u64, Schema>, groups: &mut [Vec<u64>]) {
    let group_of: HashMap<_, _> = groups
        .iter()
        .enumerate()
        .flat_map(|(group, members)| members.iter().map(move |&address| (address, group)))
        .collect();
    let mut keys = HashMap::<u64, TypeKey>::new();
    let mut arena = Vec::new();
    let mut interned = BTreeMap::new();
    let mut positions = HashMap::new();
    let mut stack = Vec::new();

    for (group, members) in groups.iter_mut().enumerate() {
        let kind = members
            .iter()
            .map(|address| schemas[address].kind)
            .min()
            .unwrap();
        let mut best: Option<(Vec<u64>, Vec<SchemaKey>)> = None;

        // Field order fixes a traversal from each root. Choose the least
        // structural key without using descriptor addresses as a tie-breaker.
        for &root in members
            .iter()
            .filter(|address| schemas[address].kind == kind)
        {
            positions.clear();
            let mut order = Vec::with_capacity(members.len());
            stack.push(root);
            while let Some(address) = stack.pop() {
                if positions.contains_key(&address) {
                    continue;
                }
                positions.insert(address, order.len());
                order.push(address);
                stack.extend(
                    schemas[&address]
                        .fields
                        .iter()
                        .rev()
                        .filter_map(|value| value.target_addr)
                        .filter(|target| group_of[target] == group),
                );
            }

            let key: Vec<_> = order
                .iter()
                .map(|address| SchemaKey {
                    kind: schemas[address].kind,
                    fields: schemas[address]
                        .fields
                        .iter()
                        .map(|value| {
                            let target = value.target_addr.map(|target| {
                                if group_of[&target] == group {
                                    TargetKey::Internal(positions[&target])
                                } else {
                                    TargetKey::External(keys[&target])
                                }
                            });
                            (value.tag, target)
                        })
                        .collect(),
                })
                .collect();
            if best
                .as_ref()
                .is_none_or(|(_, best)| compare(&key, best, &arena).is_lt())
            {
                best = Some((order, key));
            }
        }

        let (order, key) = best.unwrap();
        let group = *interned.entry(key.clone()).or_insert_with(|| {
            let group = arena.len();
            arena.push(key);
            group
        });
        for (member, &address) in order.iter().enumerate() {
            keys.insert(address, TypeKey { group, member });
        }
        *members = order;
    }
}

// Arena indices establish equality; their allocation order cannot order types.
// Compare referenced groups iteratively to also handle long dependency chains.
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

impl Transformer<'_> {
    pub fn append_types(&self, types: &mut TypeSection) {
        for group in &self.resolver.type_groups {
            types.ty().rec(group.iter().map(|address| {
                let schema = &self.resolver.schema_set[address];
                let mut fields = schema.fields.iter().map(|value| FieldType {
                    element_type: self.storage_type(value),
                    mutable: true,
                });
                let inner = match schema.kind {
                    SchemaKind::Struct => CompositeInnerType::Struct(StructType {
                        fields: fields.collect(),
                    }),
                    SchemaKind::Array => {
                        CompositeInnerType::Array(ArrayType(fields.next().unwrap()))
                    }
                };

                SubType {
                    is_final: true,
                    supertype_idxs: Vec::new(),
                    composite_type: CompositeType {
                        inner,
                        shared: false,
                        descriptor: None,
                        describes: None,
                    },
                }
            }));
        }

        for signature in &self.signatures {
            types.ty().function(
                signature.params().iter().copied(),
                signature.results().iter().copied(),
            );
        }
    }

    pub fn storage_type(&self, value: &Value) -> StorageType {
        match value.tag {
            ValueTag::I8 => StorageType::I8,
            ValueTag::I16 => StorageType::I16,
            _ => StorageType::Val(self.value_type(value)),
        }
    }

    pub fn value_type(&self, value: &Value) -> ValType {
        match value.tag {
            ValueTag::I8 | ValueTag::I16 | ValueTag::I32 | ValueTag::Bool => ValType::I32,
            ValueTag::I64 => ValType::I64,
            ValueTag::F32 => ValType::F32,
            ValueTag::F64 => ValType::F64,
            ValueTag::ExternRef => ValType::EXTERNREF,
            ValueTag::Ref | ValueTag::NullableRef | ValueTag::Array | ValueTag::NullableArray => {
                ValType::Ref(RefType {
                    nullable: matches!(value.tag, ValueTag::NullableRef | ValueTag::NullableArray),
                    heap_type: HeapType::Concrete(
                        self.resolver.type_indices
                            [&value.target_addr.expect("reference value target")],
                    ),
                })
            }
        }
    }
}
