//! A multithreaded runtime for evaluating DBSP circuits in a data-parallel
//! fashion.

use crossbeam_utils::sync::{Parker, Unparker};
use std::{
    cell::{Cell, RefCell},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::sync_channel,
        Arc,
    },
    thread::{Builder, JoinHandle, LocalKey, Result as ThreadResult},
};
use typedmap::{TypedDashMap, TypedMapKey};

// Thread-local variables used by the termination protocol.
thread_local! {
    // Parker that must be used by all schedulers within the worker
    // thread so that the scheduler gets woken up by `RuntimeHandle::kill`.
    static PARKER: Parker = Parker::new();

    // Set to `true` by `RuntimeHandle::kill`.
    // Schedulers must check this signal before evaluating each operator
    // and exit immediately returning `SchedulerError::Killed`.
    static KILL_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

// Thread-local variables used to store per-worker context.
thread_local! {
    // Reference to the `Runtime` that manages this worker thread or `None`
    // if the current thread is not running in a multithreaded runtime.
    static RUNTIME: RefCell<Option<Runtime>> = RefCell::new(None);

    // 0-based index of the current worker thread within its runtime.
    // Returns `0` if the current thread in not running in a multithreaded
    // runtime.
    static WORKER_INDEX: Cell<usize> = Cell::new(0);
}

pub struct LocalStoreMarker;

/// Local data store shared by all workers in a runtime.
pub type LocalStore = TypedDashMap<LocalStoreMarker>;

struct RuntimeInner {
    nworkers: usize,
    store: LocalStore,
}

impl RuntimeInner {
    fn new(nworkers: usize) -> Self {
        Self {
            nworkers,
            store: TypedDashMap::new(),
        }
    }
}

/// A multithreaded runtime that hosts `N` circuits running in parallel worker
/// threads. Typically, all `N` circuits are identical, but this is not required
/// or enforced.
#[repr(transparent)]
#[derive(Clone)]
pub struct Runtime(Arc<RuntimeInner>);

impl Runtime {
    /// Create a new runtime with `nworkers` worker threads and run a
    /// user-provided closure in each thread.  The closure takes a reference
    /// to the `Runtime` as an argument, so that workers can access shared
    /// services provided by the runtime.
    ///
    /// Returns a handle through which the caller can interact with the runtime.
    ///
    /// # Arguments
    ///
    /// * `nworkers` - the number of worker threads to spawn.
    ///
    /// * `f` - closure that will be invoked in each worker thread.  Normally,
    ///   this closure builds and runs a circuit.
    ///
    /// # Examples
    /// ```
    /// # #[cfg(all(windows, miri))]
    /// # fn main() {}
    ///
    /// # #[cfg(not(all(windows, miri)))]
    /// # fn main() {
    /// use dbsp::circuit::{Root, Runtime};
    ///
    /// // Create a runtime with 4 worker threads.
    /// let hruntime = Runtime::run(4, || {
    ///     // This closure runs within each worker thread.
    ///
    ///     let root = Root::build(move |circuit| {
    ///         // Populate `circuit` with operators.
    ///     })
    ///     .unwrap();
    ///
    ///     // Run circuit for 100 clock cycles.
    ///     for _ in 0..100 {
    ///         root.step().unwrap();
    ///     }
    /// });
    ///
    /// // Wait for all worker threads to terminate.
    /// hruntime.join().unwrap();
    /// # }
    /// ```
    pub fn run<F>(nworkers: usize, f: F) -> RuntimeHandle
    where
        F: FnOnce() + Clone + Send + 'static,
    {
        let mut workers = Vec::with_capacity(nworkers);

        let runtime = Self(Arc::new(RuntimeInner::new(nworkers)));

        for worker_index in 0..nworkers {
            let runtime = runtime.clone();
            let f = f.clone();
            let builder = Builder::new().name(format!("worker{}", worker_index));

            let (init_sender, init_receiver) = sync_channel(0);

            let join_handle = builder
                .spawn(move || {
                    RUNTIME.with(|rt| *rt.borrow_mut() = Some(runtime));
                    WORKER_INDEX.with(|w| w.set(worker_index));
                    init_sender
                        .send((
                            PARKER.with(|parker| parker.unparker().clone()),
                            KILL_SIGNAL.with(|s| s.clone()),
                        ))
                        .unwrap();
                    f();
                })
                .unwrap_or_else(|_| panic!("failed to spawn worker thread {}", worker_index));

            let (unparker, kill_signal) = init_receiver.recv().unwrap();
            workers.push(WorkerHandle::new(join_handle, unparker, kill_signal));
        }

        RuntimeHandle::new(runtime, workers)
    }

    /// Returns a reference to the multithreaded runtime that
    /// manages the current worker thread, or `None` if the thread
    /// runs without a runtime.
    ///
    /// Worker threads created by the [`Runtime::run`] method can access
    /// the services provided by this API via an instance of `struct Runtime`,
    /// which they can obtain by calling `Runtime::runtime()`.  DBSP circuits
    /// created without a managed runtime run in the context of the client
    /// thread.  When invoked by such a thread, this method returns `None`.
    #[allow(clippy::self_named_constructors)]
    pub fn runtime() -> Option<Runtime> {
        RUNTIME.with(|rt| rt.borrow().clone())
    }

    /// Returns 0-based index of the current worker thread within its
    /// runtime.  For threads that run without a runtime, this method
    /// returns `0`.
    pub fn worker_index() -> usize {
        WORKER_INDEX.with(|index| index.get())
    }

    fn inner(&self) -> &RuntimeInner {
        &self.0
    }

    /// Returns the number of workers in this runtime.
    pub fn num_workers(&self) -> usize {
        self.inner().nworkers
    }

    /// Returns reference to the data store shared by all workers within the
    /// runtime.
    ///
    /// This low-level mechanism can be used by various services that
    /// require common state shared across all workers.
    ///
    /// The [`LocalStore`] type is an alias to [`typedmap::TypedDashMap`], a
    /// concurrent map type that can store key/value pairs of different
    /// types.  See `typedmap` crate documentation for details.
    pub fn local_store(&self) -> &LocalStore {
        &self.inner().store
    }

    /// A per-worker sequential counter.
    ///
    /// This method can be used to generate unique identifiers that will be the
    /// same across all worker threads.  Repeated calls to this function
    /// with the same worker index generate numbers 0, 1, 2, ...
    pub fn sequence_next(&self, worker_index: usize) -> usize {
        debug_assert!(worker_index < self.inner().nworkers);
        let mut entry = self
            .local_store()
            .entry(WorkerId(worker_index))
            .or_insert(0);
        let result = *entry;
        *entry += 1;
        result
    }

    /// Returns current worker's parker to be used by schedulers.
    ///
    /// Whenever a circuit scheduler needs to block waiting for
    /// an operator to become ready, it must use this parker.
    /// This ensures that the thread will be woken up when the
    /// user tries to terminate the runtime using
    /// [`RuntimeHandle::kill`].
    pub fn parker() -> &'static LocalKey<Parker> {
        &PARKER
    }

    /// `true` if the current worker thread has received a kill signal
    /// and should exit asap.  Schedulers should use this method before
    /// scheduling the next operator and after parking.
    pub fn kill_in_progress() -> bool {
        KILL_SIGNAL.with(|signal| signal.load(Ordering::SeqCst))
    }
}

/// Per-worker controls.
struct WorkerHandle {
    join_handle: JoinHandle<()>,
    unparker: Unparker,
    kill_signal: Arc<AtomicBool>,
}

impl WorkerHandle {
    fn new(join_handle: JoinHandle<()>, unparker: Unparker, kill_signal: Arc<AtomicBool>) -> Self {
        Self {
            join_handle,
            unparker,
            kill_signal,
        }
    }
}

/// Handle returned by `Runtime::run`.
pub struct RuntimeHandle {
    runtime: Runtime,
    workers: Vec<WorkerHandle>,
}

impl RuntimeHandle {
    fn new(runtime: Runtime, workers: Vec<WorkerHandle>) -> Self {
        Self { runtime, workers }
    }

    /// Returns reference to the runtime.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Terminate the runtime and all worker threads without waiting for any
    /// in-progress computation to complete.
    ///
    /// Signals all workers to exit.  Any operators already running are
    /// evaluated to completion, after which the worker thread terminates
    /// even if the circuit has not been fully evaluated for the current
    /// clock cycle.
    pub fn kill(self) -> ThreadResult<()> {
        for worker in self.workers.iter() {
            worker.kill_signal.store(true, Ordering::SeqCst);
            worker.unparker.unpark();
        }

        self.join()
    }

    /// Wait for all workers in the runtime to terminate.
    ///
    /// The calling thread blocks until all worker threads have terminated.
    pub fn join(self) -> ThreadResult<()> {
        // Insist on joining all threads even if some of them fail.
        #[allow(clippy::needless_collect)]
        let results: Vec<ThreadResult<()>> = self
            .workers
            .into_iter()
            .map(|h| h.join_handle.join())
            .collect();
        results.into_iter().collect::<ThreadResult<()>>()
    }
}

#[derive(Hash, PartialEq, Eq)]
struct WorkerId(usize);

impl TypedMapKey<LocalStoreMarker> for WorkerId {
    type Value = usize;
}

#[cfg(test)]
mod tests {
    use super::Runtime;
    use crate::{
        circuit::{
            schedule::{DynamicScheduler, Scheduler, StaticScheduler},
            Root,
        },
        operator::Generator,
    };
    use std::{cell::RefCell, rc::Rc, thread::sleep, time::Duration};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_runtime_static() {
        test_runtime::<StaticScheduler>();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_runtime_dynamic() {
        test_runtime::<DynamicScheduler>();
    }

    fn test_runtime<S>()
    where
        S: Scheduler + 'static,
    {
        let hruntime = Runtime::run(4, || {
            let data = Rc::new(RefCell::new(vec![]));
            let data_clone = data.clone();
            let root = Root::build_with_scheduler::<_, S>(move |circuit| {
                let runtime = Runtime::runtime().unwrap();
                // Generator that produces values using `sequence_next`.
                circuit
                    .add_source(Generator::new(move || {
                        runtime.sequence_next(Runtime::worker_index())
                    }))
                    .inspect(move |n: &usize| data_clone.borrow_mut().push(*n));
            })
            .unwrap();

            for _ in 0..100 {
                root.step().unwrap();
            }

            assert_eq!(&*data.borrow(), &(0..100).collect::<Vec<usize>>());
        });

        hruntime.join().unwrap();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_kill_static() {
        test_kill::<StaticScheduler>();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_kill_dynamic() {
        test_kill::<DynamicScheduler>();
    }

    // Test `RuntimeHandle::kill`.
    fn test_kill<S>()
    where
        S: Scheduler + 'static,
    {
        let hruntime = Runtime::run(16, || {
            // Create a nested circuit that iterates forever.
            let root = Root::build_with_scheduler::<_, S>(move |circuit| {
                circuit
                    .iterate_with_scheduler::<_, _, _, S>(|child| {
                        let mut n: usize = 0;
                        child
                            .add_source(Generator::new(move || {
                                n += 1;
                                n
                            }))
                            .inspect(|_: &usize| {});
                        Ok((|| Ok(false), ()))
                    })
                    .unwrap();
            })
            .unwrap();

            loop {
                if root.step().is_err() {
                    return;
                }
            }
        });

        sleep(Duration::from_millis(100));
        hruntime.kill().unwrap();
    }
}
