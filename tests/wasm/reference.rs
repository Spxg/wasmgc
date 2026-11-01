use wasmgc::{Gc, GcArray, wasmgc};

#[wasmgc]
struct Item {
    value: i32,
}

#[wasmgc]
struct Holder {
    item: Gc<Item>,
}

#[wasmgc]
struct OptionalHolder {
    item: Option<Gc<Item>>,
}

#[wasmgc]
struct Tree {
    value: i32,
    children: GcArray<Gc<Tree>>,
}

#[wasmgc]
struct Left {
    right: Option<Gc<Right>>,
}

#[wasmgc]
struct Right {
    left: Option<Gc<Left>>,
}

#[wasmgc]
pub fn test_struct_reference_ownership() {
    let holder = Gc::new(Holder {
        item: Gc::new(Item { value: 1 }),
    });
    let original = holder.item();
    assert_eq!(original.value(), 1);

    let replacement = Gc::new(Item { value: 2 });
    holder.set_item(&replacement);
    assert!(Gc::ref_eq(&holder.item(), &replacement));

    drop(replacement);
    assert_eq!(holder.item().value(), 2);
    assert_eq!(original.value(), 1);
}

#[wasmgc]
pub fn test_struct_getter_survives_parent_drop() {
    let holder = Gc::new(Holder {
        item: Gc::new(Item { value: 3 }),
    });
    let item = holder.item();

    drop(holder);
    item.set_value(4);
    assert_eq!(item.value(), 4);
}

#[wasmgc]
pub fn test_nullable_struct_reference() {
    let holder = Gc::new(OptionalHolder { item: None });
    assert!(holder.item().is_none());

    let item = Gc::new(Item { value: 5 });
    holder.set_item(Some(&item));
    assert!(Gc::ref_eq(&holder.item().unwrap(), &item));

    drop(item);
    let retained = holder.item().unwrap();
    assert_eq!(retained.value(), 5);

    holder.set_item(None);
    assert!(holder.item().is_none());
    assert_eq!(retained.value(), 5);

    let initialized = Gc::new(OptionalHolder {
        item: Some(retained),
    });
    assert_eq!(initialized.item().unwrap().value(), 5);
}

#[wasmgc]
pub fn test_reference_array_fill_copy_and_slices() {
    let first = Gc::new(Item { value: 1 });
    let second = Gc::new(Item { value: 2 });
    let source = GcArray::array_new(67, first.clone());

    source.fill(&second);
    assert!(Gc::ref_eq(&source.get(66).unwrap(), &second));

    let input: [Gc<Item>; 67] = core::array::from_fn(|index| {
        if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        }
    });
    source.copy_from_slice(&input);
    let destination = GcArray::array_new(67, first.clone());
    destination.copy(0, &source, 0, source.len());

    let mut output: [Gc<Item>; 67] = core::array::from_fn(|_| second.clone());
    destination.copy_to_slice(&mut output);
    for (actual, expected) in output.iter().zip(&input) {
        assert!(Gc::ref_eq(actual, expected));
    }

    drop(input);
    drop(source);
    drop(destination);
    drop(first);
    drop(second);
    assert_eq!(output[0].value(), 1);
    assert_eq!(output[65].value(), 2);
    assert_eq!(output[66].value(), 1);
}

#[wasmgc]
pub fn test_struct_array_cycle() {
    let tree = Gc::new(Tree {
        value: 7,
        children: GcArray::empty(),
    });
    let children = GcArray::from_slice([tree.clone()]);
    tree.set_children(&children);

    drop(children);
    let child = tree.children().get(0).unwrap();
    assert!(Gc::ref_eq(&tree, &child));

    drop(tree);
    child.set_value(8);
    assert_eq!(child.children().get(0).unwrap().value(), 8);
}

#[wasmgc]
pub fn test_mutually_referencing_structs() {
    let left = Gc::new(Left { right: None });
    let right = Gc::new(Right { left: None });
    left.set_right(Some(&right));
    right.set_left(Some(&left));

    let roundtrip = left.right().unwrap().left().unwrap();
    assert!(Gc::ref_eq(&left, &roundtrip));

    drop(left);
    drop(right);
    let retained_right = roundtrip.right().unwrap();
    assert!(Gc::ref_eq(&roundtrip, &retained_right.left().unwrap()));

    roundtrip.set_right(None);
    retained_right.set_left(None);
    assert!(roundtrip.right().is_none());
    assert!(retained_right.left().is_none());
}

#[wasmgc]
pub fn test_nested_array_aliases() {
    type Row = GcArray<i32>;
    type Grid = GcArray<Row>;
    type Grids = GcArray<Grid>;

    let row = Row::from_slice([1, 2]);
    let grid = Grid::from_slice([row.clone()]);
    let grids = Grids::from_slice([grid.clone()]);
    let nested = grids.get(0).unwrap().get(0).unwrap();
    assert!(Gc::ref_eq(&row, &nested));

    nested.set(1, 9);
    assert_eq!(row.get(1), Some(9));

    drop(row);
    drop(grid);
    drop(grids);
    assert_eq!(nested.get(1), Some(9));
}

#[wasmgc]
pub fn test_roots_after_growth_and_raw_roundtrip() {
    let originals: Vec<_> = (0..130).map(|value| Gc::new(Item { value })).collect();
    let aliases: Vec<_> = originals.to_vec();
    drop(originals);

    let replacement = Gc::new(Item { value: -1 });
    for (value, alias) in aliases.iter().enumerate() {
        assert_eq!(alias.value(), value as i32);
    }
    assert_eq!(replacement.value(), -1);

    let item = aliases.into_iter().next().unwrap();
    let handle = item.into_raw();
    // SAFETY: into_raw transferred this live Item root, which has no other owner.
    let item = unsafe { Gc::<Item>::from_raw(handle) };
    let alias = item.clone();
    drop(item);

    alias.set_value(11);
    assert_eq!(alias.value(), 11);
}
