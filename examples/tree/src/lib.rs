use wasmgc::{Gc, wasmgc};

/// A node shared by immutable versions of a binary tree.
#[wasmgc]
pub struct Node {
    #[getter]
    value: i32,
    #[getter]
    left: Option<Gc<Node>>,
    #[getter]
    right: Option<Gc<Node>>,
}

/// Creates a complete binary tree with values numbered 1 through 15 by level.
#[wasmgc]
pub fn create_tree() -> Gc<Node> {
    fn build(index: u32) -> Gc<Node> {
        let (left, right) = if index < 8 {
            (Some(build(index * 2)), Some(build(index * 2 + 1)))
        } else {
            (None, None)
        };

        Gc::new(Node {
            value: index as i32,
            left,
            right,
        })
    }

    build(1)
}

/// Copies the path to `index`, preserving the original tree and shared subtrees.
///
/// Indices use heap order: 1 is the root, 2 its left child, and 3 its right child.
///
/// # Panics
///
/// Panics if `index` is zero or does not identify a node in `root`.
#[wasmgc]
pub fn edit_tree(root: &Gc<Node>, index: u32, value: i32) -> Gc<Node> {
    fn copy_path(node: &Gc<Node>, index: u32, bit: u32, value: i32) -> Gc<Node> {
        let mut next = Node {
            value: if bit == 0 { value } else { node.value() },
            left: node.left(),
            right: node.right(),
        };

        if bit != 0 {
            let child = if index & bit == 0 {
                &mut next.left
            } else {
                &mut next.right
            };
            *child = Some(copy_path(child.as_ref().unwrap(), index, bit >> 1, value));
        }

        Gc::new(next)
    }

    copy_path(root, index, (1_u32 << index.ilog2()) >> 1, value)
}
