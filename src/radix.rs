/// A simple lock-free radix tree, assumes a dense keyspace.
use std::sync::atomic::{ATOMIC_USIZE_INIT, AtomicUsize};
use std::sync::atomic::Ordering::{Acquire, Relaxed, SeqCst};

use sled_sync::{Atomic, Guard, Owned, Shared, pin, unprotected};

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
        let guard = pin();
        unsafe {
            let children: Vec<*const Node> = self.children
                .iter()
                .map(|c| c.load(Acquire, &guard).as_raw())
                .filter(|c| !c.is_null())
                .collect();

            for child in children {
                drop(Box::from_raw(child as *mut Node));
            }
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
            head: Atomic::from(head),
        }
    }
}

impl Drop for Radix {
    fn drop(&mut self) {
        unsafe {
            let guard = unprotected();
            let head = self.head.load(Acquire, guard).as_raw();
            drop(Box::from_raw(head as *mut Node));
        }
    }
}

impl Radix {
    /// Try to get a value from the tree.
    pub fn get(&self, id: u16) -> usize {
        let guard = pin();
        unsafe {
            let tip = traverse(self.head.load(Acquire, &guard), id, true, &guard);
            tip.deref().inner.load(Acquire)
        }
    }

    /// Increment a value.
    pub fn incr(&self, id: u16) -> usize {
        let guard = pin();
        unsafe {
            let tip = traverse(self.head.load(Acquire, &guard), id, true, &guard);
            tip.deref().inner.fetch_add(1, Relaxed) + 1
        }
    }
}

#[inline(always)]
fn traverse<'s>(
    ptr: Shared<'s, Node>,
    id: u16,
    create_intermediate: bool,
    guard: &'s Guard,
) -> Shared<'s, Node> {
    if id == 0 {
        return ptr;
    }

    let (first_bits, remainder) = split_fanout(id as usize);
    let child_index = first_bits;
    let children = unsafe { &ptr.deref().children };
    let mut next_ptr = children[child_index].load(Acquire, guard);

    if next_ptr.is_null() {
        if !create_intermediate {
            return Shared::null();
        }

        let next_child = Owned::new(Node::default()).into_shared(guard);
        let ret = children[child_index].compare_and_set(next_ptr, next_child.clone(), SeqCst, guard);
        if ret.is_ok() {
            next_ptr = next_child;
        } else {
            unsafe {
                // must clean up the memory we failed to CAS in
                // so it doesn't leak.
                drop(next_child.into_owned())
            }
            next_ptr = ret.unwrap_err().current;
        }
    }

    traverse(next_ptr, remainder as u16, create_intermediate, guard)
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
