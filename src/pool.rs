//! Persistent helper threads for candidate-parallel batches.
//!
//! Creating and joining scoped threads for every batch costs far more than a
//! small batch of domain evaluations on some hosts. Helpers created here live
//! for the rest of the process and wait on a condition variable between
//! batches. The calling thread always works on its own batch, so a batch makes
//! progress even when no helper is free.

use std::{
    panic::{self, AssertUnwindSafe},
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
};

/// Upper bound on helper threads, matching the 64-worker budget limit.
const MAX_HELPERS: usize = 63;

type Task = &'static (dyn Fn() + Sync);

struct Entry {
    id: u64,
    task: Task,
    /// Helpers that may still join this batch.
    wanted: usize,
    /// Helpers currently running `task`.
    active: usize,
    panicked: bool,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    next_id: u64,
    helpers: usize,
}

struct Pool {
    state: Mutex<State>,
    work: Condvar,
    done: Condvar,
}

fn lock(pool: &Pool) -> MutexGuard<'_, State> {
    pool.state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Return the pool owned by the current process. A forked child does not
/// inherit the parent's threads, so it gets a fresh pool.
fn pool() -> &'static Pool {
    static POOLS: Mutex<Option<(u32, &'static Pool)>> = Mutex::new(None);
    let pid = std::process::id();
    let mut current = POOLS.lock().unwrap_or_else(PoisonError::into_inner);
    match *current {
        Some((owner, pool)) if owner == pid => pool,
        _ => {
            let pool: &'static Pool = Box::leak(Box::new(Pool {
                state: Mutex::new(State::default()),
                work: Condvar::new(),
                done: Condvar::new(),
            }));
            *current = Some((pid, pool));
            pool
        }
    }
}

fn helper(pool: &'static Pool) {
    let mut state = lock(pool);
    loop {
        let Some(entry) = state.entries.iter_mut().find(|e| e.wanted > 0) else {
            state = pool
                .work
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
            continue;
        };
        entry.wanted -= 1;
        entry.active += 1;
        let (id, task) = (entry.id, entry.task);
        drop(state);
        let ok = panic::catch_unwind(AssertUnwindSafe(task)).is_ok();
        state = lock(pool);
        let entry = state
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .expect("a batch outlives its active helpers");
        entry.active -= 1;
        entry.panicked |= !ok;
        if entry.active == 0 {
            pool.done.notify_all();
        }
    }
}

/// Run `task` on the calling thread and on up to `helpers` pool threads at
/// once, and return after every participant has left `task`. `task` must
/// share its work through its own synchronization, for example an atomic
/// index. A panic in any participant is propagated to the caller.
pub fn run(helpers: usize, task: &(dyn Fn() + Sync)) {
    let helpers = helpers.min(MAX_HELPERS);
    if helpers == 0 {
        task();
        return;
    }
    let pool = pool();
    // SAFETY: the erased reference is only reachable through this batch's
    // entry. Helpers copy it while holding the lock and count themselves in
    // `active`. Before returning or unwinding, this function closes the entry
    // and waits under the same lock until `active` is zero, so no helper can
    // use `task` after it goes out of scope.
    let erased: Task = unsafe { std::mem::transmute::<&(dyn Fn() + Sync), Task>(task) };
    let id = {
        let mut state = lock(pool);
        let id = state.next_id;
        state.next_id += 1;
        state.entries.push(Entry {
            id,
            task: erased,
            wanted: helpers,
            active: 0,
            panicked: false,
        });
        let demand = state
            .entries
            .iter()
            .map(|e| e.wanted + e.active)
            .sum::<usize>()
            .min(MAX_HELPERS);
        while state.helpers < demand {
            let spawned = std::thread::Builder::new()
                .name("hydro-craft-worker".into())
                .spawn(move || helper(pool));
            if spawned.is_err() {
                break;
            }
            state.helpers += 1;
        }
        for _ in 0..helpers {
            pool.work.notify_one();
        }
        id
    };
    let mine = panic::catch_unwind(AssertUnwindSafe(task));
    let mut state = lock(pool);
    let position = |state: &State| state.entries.iter().position(|e| e.id == id).unwrap();
    let index = position(&state);
    state.entries[index].wanted = 0;
    while state.entries[position(&state)].active > 0 {
        state = pool
            .done
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner);
    }
    let index = position(&state);
    let entry = state.entries.swap_remove(index);
    drop(state);
    if let Err(payload) = mine {
        panic::resume_unwind(payload);
    }
    assert!(!entry.panicked, "a pool worker panicked");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn every_index_runs_once_across_repeated_and_concurrent_batches() {
        for helpers in [0, 1, 3, 7, 200] {
            let next = AtomicUsize::new(0);
            let hits: Vec<_> = (0..500).map(|_| AtomicUsize::new(0)).collect();
            run(helpers, &|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(hit) = hits.get(i) else { break };
                    hit.fetch_add(1, Ordering::Relaxed);
                }
            });
            assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1));
        }
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..50 {
                        let total = AtomicUsize::new(0);
                        let next = AtomicUsize::new(0);
                        run(3, &|| {
                            while next.fetch_add(1, Ordering::Relaxed) < 64 {
                                total.fetch_add(1, Ordering::Relaxed);
                            }
                        });
                        assert_eq!(total.load(Ordering::Relaxed), 64);
                    }
                });
            }
        });
    }

    #[test]
    fn panics_reach_the_caller_and_the_pool_keeps_working() {
        let next = AtomicUsize::new(0);
        let outcome = panic::catch_unwind(|| {
            run(2, &|| {
                if next.fetch_add(1, Ordering::Relaxed) < 3 {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    panic!("boom");
                }
            })
        });
        assert!(outcome.is_err());
        let count = AtomicUsize::new(0);
        run(2, &|| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        assert!((1..=3).contains(&count.load(Ordering::Relaxed)));
    }
}
