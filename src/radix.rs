/// A simple lock-free radix tree, assumes a dense keyspace.
use std::sync::atomic::{ATOMIC_USIZE_INIT, AtomicUsize};
use std::sync::atomic::Ordering::{Acquire, Relaxed, SeqCst};

use coco::epoch::{Atomic, Owned, Ptr, Scope, pin, unprotected};

const FANFACTOR: usize = 6;
const FANOUT: usize = 1 << FANFACTOR;
const FAN_MASK: usize = FANOUT - 1;

#[inline(always)]
fn split_fanout(i: usize) -> (usize, usize) {
    let rem = i >> FANFACTOR;
    let first = i & FAN_MASK;
    (first, rem)
}

struct Node {
    inner: AtomicUsize,
    children: Vec<Atomic<Node>>,
}

unsafe impl Send for Node {}

unsafe impl Sync for Node {}

impl Default for Node {
    fn default() -> Node {
        let children = rep_no_copy!(Atomic::null(); FANOUT);
        Node {
            inner: ATOMIC_USIZE_INIT,
            children: children,
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        unsafe {
            pin(|scope| {
                let children: Vec<*const Node> = self.children
                    .iter()
                    .map(|c| c.load(Acquire, scope).as_raw())
                    .filter(|c| !c.is_null())
                    .collect();

                for child in children {
                    drop(Box::from_raw(child as *mut Node));
                }
            })
        }
    }
}

/// A simple lock-free radix tree.
pub struct Radix {
    head: Atomic<Node>,
}

impl Default for Radix {
    fn default() -> Radix {
        let head = Owned::new(Node::default());
        Radix {
            head: Atomic::from_owned(head),
        }
    }
}

impl Drop for Radix {
    fn drop(&mut self) {
        unsafe {
            unprotected(|scope| {
                let head = self.head.load(Acquire, scope).as_raw();
                drop(Box::from_raw(head as *mut Node));
            })
        }
    }
}

impl Radix {
    /// Try to get a value from the tree.
    pub fn get(&self, id: u16) -> usize {
        unsafe {
            unprotected(|scope| {
                let tip = traverse(self.head.load(Acquire, scope), id, true, scope);
                tip.deref().inner.load(Acquire)
            })
        }
    }

    /// Increment a value.
    pub fn incr(&self, id: u16) -> usize {
        unsafe {
            unprotected(|scope| {
                let tip = traverse(self.head.load(Acquire, scope), id, true, scope);
                tip.deref().inner.fetch_add(1, Relaxed) + 1
            })
        }
    }
}

#[inline(always)]
fn traverse<'s>(
    ptr: Ptr<'s, Node>,
    id: u16,
    create_intermediate: bool,
    scope: &'s Scope,
) -> Ptr<'s, Node> {
    if id == 0 {
        return ptr;
    }

    let (first_bits, remainder) = split_fanout(id as usize);
    let child_index = first_bits;
    let children = unsafe { &ptr.deref().children };
    let mut next_ptr = children[child_index].load(Acquire, scope);

    if next_ptr.is_null() {
        if !create_intermediate {
            return Ptr::null();
        }

        let next_child = Owned::new(Node::default());
        match children[child_index].compare_and_swap_owned(next_ptr, next_child, SeqCst, scope) {
            Err((actual, failure_child)) => {
                // another thread beat us, drop unused created
                // child and use what is already set
                next_ptr = actual;
                drop(failure_child);
            }
            Ok(next_child) => {
                // CAS worked
                next_ptr = next_child;
            }
        }
    }

    traverse(next_ptr, remainder as u16, create_intermediate, scope)
}

#[test]
fn test_split_fanout() {
    assert_eq!(split_fanout(0b11_1111), (0b11_1111, 0));
    assert_eq!(split_fanout(0b111_1111), (0b11_1111, 0b1));
}

#[test]
fn basic_functionality() {
    let rt = Radix::default();
    rt.incr(16);
    rt.incr(16);
    rt.incr(16);
    rt.incr(16);
    rt.incr(16);

    let count = rt.get(16);
    assert_eq!(count, 5);
}
