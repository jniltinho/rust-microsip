//! Bounded, lock-free single-producer/single-consumer ring of `i16` samples.
//!
//! Built from atomics only (no `unsafe`): every slot is an [`AtomicI16`], and
//! the producer and consumer each own one monotonically increasing index. The
//! split into [`Producer`] and [`Consumer`] handles (neither is `Clone`)
//! enforces the single-producer/single-consumer discipline at the type level.
//! Neither side ever blocks or allocates after construction. A full ring
//! rejects the newest sample; the caller counts the drop.

use std::sync::Arc;
use std::sync::atomic::{AtomicI16, AtomicUsize, Ordering};

struct Shared {
    slots: Box<[AtomicI16]>,
    /// Next index to read; written only by the consumer.
    head: AtomicUsize,
    /// Next index to write; written only by the producer.
    tail: AtomicUsize,
}

/// Writing half of a ring created by [`ring`].
pub(crate) struct Producer(Arc<Shared>);

/// Reading half of a ring created by [`ring`].
pub(crate) struct Consumer(Arc<Shared>);

/// Allocate a ring holding up to `capacity` samples (at least one).
pub(crate) fn ring(capacity: usize) -> (Producer, Consumer) {
    let capacity = capacity.max(1);
    let shared = Arc::new(Shared {
        slots: (0..capacity).map(|_| AtomicI16::new(0)).collect(),
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (Producer(Arc::clone(&shared)), Consumer(shared))
}

// ponytail: indices are plain usize counters; with a non-power-of-two capacity
// the modulo mapping breaks only after 2^64 samples (millions of years at 48 kHz).
impl Producer {
    /// Append one sample. Returns `false` (and drops the sample) when full.
    pub(crate) fn push(&mut self, sample: i16) -> bool {
        let s = &self.0;
        let tail = s.tail.load(Ordering::Relaxed);
        let head = s.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= s.slots.len() {
            return false;
        }
        if let Some(slot) = s.slots.get(tail % s.slots.len()) {
            slot.store(sample, Ordering::Relaxed);
        }
        s.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Samples currently queued (a lower bound while the consumer runs).
    pub(crate) fn len(&self) -> usize {
        let s = &self.0;
        s.tail
            .load(Ordering::Relaxed)
            .wrapping_sub(s.head.load(Ordering::Acquire))
    }
}

impl Consumer {
    /// Remove the oldest sample, or `None` when empty.
    pub(crate) fn pop(&mut self) -> Option<i16> {
        let s = &self.0;
        let head = s.head.load(Ordering::Relaxed);
        if head == s.tail.load(Ordering::Acquire) {
            return None;
        }
        let value = s
            .slots
            .get(head % s.slots.len())
            .map_or(0, |slot| slot.load(Ordering::Relaxed));
        s.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    /// Samples currently queued (a lower bound while the producer runs).
    pub(crate) fn len(&self) -> usize {
        let s = &self.0;
        s.tail
            .load(Ordering::Acquire)
            .wrapping_sub(s.head.load(Ordering::Relaxed))
    }

    /// Ring capacity in samples.
    pub(crate) fn capacity(&self) -> usize {
        self.0.slots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_order_bounded_capacity_and_wraparound() {
        let (mut tx, mut rx) = ring(3);
        assert_eq!(rx.pop(), None);
        for round in 0..5_i16 {
            assert!(tx.push(round * 10));
            assert!(tx.push(round * 10 + 1));
            assert!(tx.push(round * 10 + 2));
            assert!(!tx.push(99), "full ring must reject the newest sample");
            assert_eq!(tx.len(), 3);
            assert_eq!(rx.pop(), Some(round * 10));
            assert_eq!(rx.pop(), Some(round * 10 + 1));
            assert_eq!(rx.pop(), Some(round * 10 + 2));
            assert_eq!(rx.pop(), None);
        }
    }

    #[test]
    fn concurrent_producer_and_consumer_preserve_sequence() {
        let (mut tx, mut rx) = ring(64);
        const N: i32 = 200_000;
        let producer = std::thread::spawn(move || {
            let mut next = 0_i32;
            while next < N {
                if tx.push((next % 30_000) as i16) {
                    next += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let mut expected = 0_i32;
        while expected < N {
            match rx.pop() {
                Some(v) => {
                    assert_eq!(v, (expected % 30_000) as i16);
                    expected += 1;
                }
                None => std::thread::yield_now(),
            }
        }
        assert!(producer.join().is_ok());
    }
}
