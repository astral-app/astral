use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex, Once};
use std::time::{Duration, Instant};

use crate::lua_api as lua;
use crate::lua_host as host;

struct Queue {
    due: BTreeMap<(Instant, u64), u64>,
    sequence: u64,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue {
    due: BTreeMap::new(),
    sequence: 0,
});
static WAKE: Condvar = Condvar::new();
static THREAD: Once = Once::new();

fn queue() -> std::sync::MutexGuard<'static, Queue> {
    QUEUE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn run() {
    let mut guard = queue();
    loop {
        let Some((&(next, _), _)) = guard.due.iter().next() else {
            guard = WAKE.wait(guard).unwrap_or_else(|poisoned| poisoned.into_inner());
            continue;
        };
        let now = Instant::now();
        if now < next {
            guard = WAKE
                .wait_timeout(guard, next - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
            continue;
        }
        let mut due = Vec::new();
        while let Some((&key, &identifier)) = guard.due.iter().next() {
            if key.0 > now {
                break;
            }
            guard.due.remove(&key);
            due.push(identifier);
        }
        drop(guard);
        for identifier in due {
            fire(identifier);
        }
        guard = queue();
    }
}

pub fn schedule(identifier: u64, delay: i64) {
    THREAD.call_once(|| {
        std::thread::spawn(run);
    });
    let mut guard = queue();
    guard.sequence += 1;
    let sequence = guard.sequence;
    guard
        .due
        .insert((Instant::now() + Duration::from_millis(delay.max(0) as u64), sequence), identifier);
    WAKE.notify_one();
}

fn fire(identifier: u64) {
    let _held = host::lock();
    for plugin in host::plugins() {
        let Some(timer) = (unsafe { &*plugin }).timers.get(&identifier).copied() else {
            continue;
        };
        if timer.interval == 0 {
            unsafe { (*plugin).timers.remove(&identifier) };
        }
        if !unsafe { (*plugin).running() } {
            return;
        }
        let state = unsafe { (*plugin).state() };
        let top = unsafe { lua::lua_gettop(state) };
        unsafe { host::push_callback(plugin, timer.reference, timer.interval == 0) };
        if !host::call(plugin, 0, 0, "timer") {
            unsafe { lua::lua_settop(state, top) };
            host::fail_and_stop(plugin);
            return;
        }
        unsafe { lua::lua_settop(state, top) };
        if timer.interval != 0 && unsafe { (*plugin).timers.contains_key(&identifier) } {
            schedule(identifier, timer.interval);
        }
        return;
    }
}
