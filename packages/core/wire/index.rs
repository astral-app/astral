use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Mutex;

use super::format::{Span, child};

const REMEMBERED_BATCHES: usize = 4;
pub const INDEXED_SPANS: usize = 96;

#[derive(Default)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write_i32(&mut self, value: i32) {
        self.0 = u64::from(value as u32).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type IdMap<V> = HashMap<i32, V, BuildHasherDefault<IdHasher>>;

#[derive(Default)]
pub struct BatchIndex {
    creations: IdMap<usize>,
    child_lists: IdMap<usize>,
    parents: IdMap<i32>,
}

impl BatchIndex {
    fn clear(&mut self) {
        self.creations.clear();
        self.child_lists.clear();
        self.parents.clear();
    }

    fn fill(&mut self, bytes: &[u8], spans: &[Span]) {
        let index = self;
        for (position, span) in spans.iter().enumerate() {
            match span.op {
                1 => {
                    index.creations.entry(span.id).or_insert(position);
                }
                3 => {
                    index.child_lists.insert(span.id, position);
                    for slot in 0..span.child_count {
                        index.parents.insert(child(bytes, span, slot), span.id);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn creation(&self, id: i32) -> Option<usize> {
        self.creations.get(&id).copied()
    }

    pub fn child_list(&self, id: i32) -> Option<usize> {
        self.child_lists.get(&id).copied()
    }

    pub fn parent(&self, id: i32) -> i32 {
        self.parents.get(&id).copied().unwrap_or(0)
    }
}

struct Remembered {
    spans: usize,
    count: usize,
    index: BatchIndex,
}

static BATCHES: Mutex<Vec<Remembered>> = Mutex::new(Vec::new());

pub fn remember(spans: *const Span, count: usize, bytes: &[u8], scanned: &[Span]) {
    let Ok(mut batches) = BATCHES.lock() else { return };
    if count < INDEXED_SPANS {
        batches.retain(|batch| batch.spans != spans as usize);
        return;
    }
    let position = batches.iter().position(|batch| batch.spans == spans as usize);
    let mut entry = match position {
        Some(position) => batches.remove(position),
        None if batches.len() == REMEMBERED_BATCHES => batches.remove(0),
        None => Remembered {
            spans: 0,
            count: 0,
            index: BatchIndex::default(),
        },
    };
    entry.spans = spans as usize;
    entry.count = count;
    entry.index.clear();
    entry.index.fill(bytes, scanned);
    batches.push(entry);
}

pub fn forget(spans: *const Span) {
    if let Ok(mut batches) = BATCHES.lock() {
        batches.retain(|batch| batch.spans != spans as usize);
    }
}

pub fn with_index<T>(spans: *const Span, count: usize, lookup: impl FnOnce(&BatchIndex) -> T) -> Option<T> {
    let batches = BATCHES.lock().ok()?;
    batches
        .iter()
        .find(|batch| batch.spans == spans as usize && batch.count == count)
        .map(|batch| lookup(&batch.index))
}
