//! Global allocator: power-of-two size classes (16 B .. 64 KiB) with lock-free thread-local free lists
//! over the system allocator. Why: musl's mallocng takes one process-wide lock on every malloc/free
//! (50 concurrent clones → ~16k futex calls a workload) and unmaps emptied groups (~3 mmap/munmap
//! per push request). Production runs under gVisor, where futex, mmap and page faults are costly.
//! Blocks are never returned to the OS; memory is bounded by the peak live size per class.
//! Threads spill full lists to a global per-class stack of batches; `flush` hands a dying thread's
//! cache over (http.rs calls it when a connection thread ends).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::ptr::null_mut;
use std::sync::Mutex;

const MIN_SHIFT: usize = 4;
const CLASSES: usize = 13;
const MAX: usize = 1 << (MIN_SHIFT + CLASSES - 1);

/// A free block. `batch` is used only by the head of a batch on the global stack.
struct Node {
    next: *mut Node,
    batch: *mut Node,
}

struct Local {
    head: [*mut Node; CLASSES],
    n: [usize; CLASSES],
}

thread_local! {
    static LOCAL: UnsafeCell<Local> = const { UnsafeCell::new(Local { head: [null_mut(); CLASSES], n: [0; CLASSES] }) };
}

struct Batches(*mut Node);
// SAFETY: the pointers are free blocks owned by whoever holds the lock.
unsafe impl Send for Batches {}

static GLOBAL: [Mutex<Batches>; CLASSES] = [const { Mutex::new(Batches(null_mut())) }; CLASSES];

/// Blocks a thread keeps per class before spilling them as one batch (~32 KiB, 4..64 blocks).
fn limit(c: usize) -> usize {
    ((32 << 10) >> (c + MIN_SHIFT)).clamp(4, 64)
}

fn class(size: usize, align: usize) -> Option<usize> {
    if align > 16 || size > MAX {
        return None;
    }
    Some((size.max(1).next_power_of_two().trailing_zeros() as usize).saturating_sub(MIN_SHIFT))
}

fn block_layout(c: usize) -> Layout {
    // SAFETY: power of two size ≥ 16, align 16.
    unsafe { Layout::from_size_align_unchecked(1 << (c + MIN_SHIFT), 16) }
}

fn global(c: usize) -> std::sync::MutexGuard<'static, Batches> {
    GLOBAL[c].lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Alloc;

unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some(c) = class(layout.size(), layout.align()) else { return unsafe { System.alloc(layout) } };
        LOCAL.with(|l| {
            let l = unsafe { &mut *l.get() };
            if l.head[c].is_null() {
                let b = {
                    let mut g = global(c);
                    let b = g.0;
                    if !b.is_null() {
                        g.0 = unsafe { (*b).batch };
                    }
                    b
                };
                if b.is_null() {
                    return unsafe { System.alloc(block_layout(c)) };
                }
                let (mut n, mut p) = (0, b);
                while !p.is_null() {
                    n += 1;
                    p = unsafe { (*p).next };
                }
                l.head[c] = b;
                l.n[c] = n;
            }
            let b = l.head[c];
            l.head[c] = unsafe { (*b).next };
            l.n[c] -= 1;
            b as *mut u8
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let Some(c) = class(layout.size(), layout.align()) else { return unsafe { System.dealloc(ptr, layout) } };
        LOCAL.with(|l| {
            let l = unsafe { &mut *l.get() };
            if l.n[c] >= limit(c) {
                spill(l, c);
            }
            let b = ptr as *mut Node;
            unsafe { (*b).next = l.head[c] };
            l.head[c] = b;
            l.n[c] += 1;
        })
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = class(layout.size(), layout.align());
        let new = class(new_size, layout.align());
        match (old, new) {
            (Some(a), Some(b)) if a == b => ptr,
            (None, None) => unsafe { System.realloc(ptr, layout, new_size) },
            _ => unsafe {
                let p = self.alloc(Layout::from_size_align_unchecked(new_size, layout.align()));
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(ptr, p, layout.size().min(new_size));
                    self.dealloc(ptr, layout);
                }
                p
            },
        }
    }
}

fn spill(l: &mut Local, c: usize) {
    let b = l.head[c];
    if b.is_null() {
        return;
    }
    let mut g = global(c);
    unsafe { (*b).batch = g.0 };
    g.0 = b;
    l.head[c] = null_mut();
    l.n[c] = 0;
}

/// Give this thread's cached blocks to the global lists (call before a thread exits).
pub fn flush() {
    LOCAL.with(|l| {
        let l = unsafe { &mut *l.get() };
        for c in 0..CLASSES {
            spill(l, c);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_and_reuse() {
        assert_eq!(class(0, 1), Some(0));
        assert_eq!(class(16, 8), Some(0));
        assert_eq!(class(17, 8), Some(1));
        assert_eq!(class(MAX, 16), Some(CLASSES - 1));
        assert_eq!(class(MAX + 1, 16), None);
        assert_eq!(class(8, 32), None);
        // Cross-thread frees, spills and flushes keep every block intact.
        let hs: Vec<_> = (0..8)
            .map(|t| {
                std::thread::spawn(move || {
                    let mut v: Vec<Vec<u8>> = (0..500).map(|i| vec![t as u8; (i * 37) % 5000]).collect();
                    for (i, b) in v.iter_mut().enumerate() {
                        b.extend_from_slice(&[t as u8; 100]);
                        assert!(b.iter().all(|&x| x == t as u8), "{i}");
                    }
                    flush();
                    v
                })
            })
            .collect();
        let all: Vec<Vec<Vec<u8>>> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        for (t, v) in all.iter().enumerate() {
            assert!(v.iter().all(|b| b.iter().all(|&x| x == t as u8)));
        }
        drop(all);
        flush();
    }
}
