use crossbeam::channel::{unbounded, Receiver, Sender};
use std::marker::PhantomData;
use std::thread;

type Task = Box<dyn FnOnce() + Send + 'static>;

/// A concurrent task executor based on a thread pool pattern.
/// Tasks can be submitted from any thread and will be executed by worker threads.
pub struct Executor {
    sender: Sender<Message>,
    workers: Vec<Worker>,
}

enum Message {
    Task(Task),
    Shutdown,
}

struct Worker {
    id: usize,
    handle: Option<thread::JoinHandle<()>>,
}

impl Executor {
    /// Creates a new executor with the specified number of worker threads.
    pub fn new(size: usize) -> Self {
        assert!(size > 0, "Thread pool size must be greater than 0");

        let (sender, receiver) = unbounded();
        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, receiver.clone()));
        }

        Executor { sender, workers }
    }

    // Creates a single-threaded executor.
    pub fn single_threaded() -> Self {
        Self::new(1)
    }

    /// Executes a task on the thread pool.
    /// Tasks are executed in FIFO order, but completion order is non-deterministic.
    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let task = Box::new(f);
        self.sender.send(Message::Task(task)).unwrap();
    }

    /// Spawns a task and returns a future that resolves to the task's result.
    /// The caller can wait on the TaskFuture to get the result.
    pub fn spawn<F, T>(&self, f: F) -> TaskFuture<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = crossbeam::channel::bounded(1);

        let task = Box::new(move || {
            let result = f();
            let _ = tx.send(result);
        });

        self.sender.send(Message::Task(task)).unwrap();

        TaskFuture { receiver: rx }
    }

    /// Returns a handle that can be used to submit tasks from other threads.
    pub fn handle(&self) -> ExecutorHandle {
        ExecutorHandle {
            sender: self.sender.clone(),
        }
    }

    /// Returns the number of worker threads in the pool.
    pub fn size(&self) -> usize {
        self.workers.len()
    }

    /// Creates a scope for spawning tasks that can access non-'static data.
    /// The scope ensures all spawned tasks complete before returning.
    ///
    /// # Example
    /// ```ignore
    /// let mut data = vec![1, 2, 3, 4];
    /// executor.scope(|s| {
    ///     for item in &mut data {
    ///         s.spawn(|| {
    ///             *item *= 2;
    ///         });
    ///     }
    /// });
    /// // All tasks guaranteed to be complete here
    /// assert_eq!(data, vec![2, 4, 6, 8]);
    /// ```
    pub fn scope<'env, F, R>(&'env self, f: F) -> R
    where
        F: FnOnce(&Scope<'env>) -> R,
    {
        let scope = Scope {
            executor: self,
            _phantom: PhantomData,
        };

        f(&scope)
    }
}

/// A scope for spawning tasks that can borrow non-'static data.
/// All tasks spawned within the scope are guaranteed to complete before the scope ends.
pub struct Scope<'env> {
    executor: &'env Executor,
    _phantom: PhantomData<std::cell::Cell<&'env ()>>,
}

impl<'env> Scope<'env> {
    /// Spawns a scoped task that can access data from the environment.
    /// The task must complete before the scope ends.
    pub fn spawn<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'env,
    {
        // Safety: We transmute the lifetime to 'static for storage in the executor.
        // This is safe because:
        // 1. The Scope holds a reference to the Executor, preventing it from being dropped
        // 2. We wait for all tasks to complete in the drop impl
        // 3. The tasks cannot outlive the scope due to the lifetime constraint
        let task: Box<dyn FnOnce() + Send + 'env> = Box::new(f);
        let static_task: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(task) };

        self.executor
            .sender
            .send(Message::Task(static_task))
            .unwrap();
    }

    /// Spawns a scoped task and returns a future for its result.
    pub fn spawn_with_result<F, T>(&self, f: F) -> TaskFuture<T>
    where
        F: FnOnce() -> T + Send + 'env,
        T: Send + 'static,
    {
        let (tx, rx) = crossbeam::channel::bounded(1);

        // Safety: Same as spawn() - the scope ensures task completion
        let task: Box<dyn FnOnce() + Send + 'env> = Box::new(move || {
            let result = f();
            let _ = tx.send(result);
        });
        let static_task: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(task) };

        self.executor
            .sender
            .send(Message::Task(static_task))
            .unwrap();

        TaskFuture { receiver: rx }
    }
}

impl<'env> Drop for Scope<'env> {
    fn drop(&mut self) {
        // Wait for all tasks to complete by sending a marker task and waiting for it
        let (tx, rx) = crossbeam::channel::bounded::<()>(1);

        // Send a marker task to each worker
        for _ in 0..self.executor.workers.len() {
            let tx = tx.clone();
            let task: Task = Box::new(move || {
                let _ = tx.send(());
            });
            self.executor.sender.send(Message::Task(task)).unwrap();
        }

        // Wait for all workers to process their marker
        for _ in 0..self.executor.workers.len() {
            let _ = rx.recv();
        }
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        // Send shutdown message to all workers
        for _ in &self.workers {
            self.sender.send(Message::Shutdown).unwrap();
        }

        // Wait for all workers to finish
        for worker in &mut self.workers {
            if let Some(handle) = worker.handle.take() {
                handle.join().unwrap();
            }
        }
    }
}

impl Worker {
    fn new(id: usize, receiver: crossbeam::channel::Receiver<Message>) -> Self {
        let handle = thread::spawn(move || loop {
            match receiver.recv() {
                Ok(Message::Task(task)) => {
                    task();
                }
                Ok(Message::Shutdown) => {
                    break;
                }
                Err(_) => {
                    // Channel disconnected, exit
                    break;
                }
            }
        });

        Worker {
            id,
            handle: Some(handle),
        }
    }
}

/// A handle to submit tasks to an executor from other threads.
/// Clone this handle to share it across threads.
#[derive(Clone)]
pub struct ExecutorHandle {
    sender: Sender<Message>,
}

impl ExecutorHandle {
    /// Executes a task on the thread pool.
    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let task = Box::new(f);
        self.sender.send(Message::Task(task)).unwrap();
    }

    /// Spawns a task and returns a future that resolves to the task's result.
    pub fn spawn<F, T>(&self, f: F) -> TaskFuture<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = crossbeam::channel::bounded(1);

        let task = Box::new(move || {
            let result = f();
            let _ = tx.send(result);
        });

        self.sender.send(Message::Task(task)).unwrap();

        TaskFuture { receiver: rx }
    }
}

/// A future representing the result of a spawned task.
/// Use `wait()` to block until the task completes and get its result.
pub struct TaskFuture<T> {
    receiver: Receiver<T>,
}

impl<T> TaskFuture<T> {
    /// Waits for the task to complete and returns its result.
    /// This blocks the current thread until the task finishes execution.
    pub fn wait(self) -> Result<T, TaskError> {
        self.receiver.recv().map_err(|_| TaskError::TaskFailed)
    }

    /// Attempts to get the result without blocking.
    /// Returns `Ok(Some(result))` if ready, `Ok(None)` if not ready yet,
    /// or `Err` if the task failed.
    pub fn try_wait(&self) -> Result<Option<T>, TaskError> {
        match self.receiver.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(crossbeam::channel::TryRecvError::Empty) => Ok(None),
            Err(crossbeam::channel::TryRecvError::Disconnected) => Err(TaskError::TaskFailed),
        }
    }
}

/// Error type for task execution failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskError {
    /// The task failed to complete (executor was dropped or task panicked).
    TaskFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn test_executor_executes_tasks() {
        let executor = Executor::new(4);
        let counter = Arc::new(Mutex::new(0));

        for _ in 0..10 {
            let counter = Arc::clone(&counter);
            executor.execute(move || {
                let mut num = counter.lock().unwrap();
                *num += 1;
            });
        }

        // Give tasks time to complete
        thread::sleep(Duration::from_millis(100));

        assert_eq!(*counter.lock().unwrap(), 10);
    }

    #[test]
    fn test_executor_handle_from_multiple_threads() {
        let executor = Executor::new(2);
        let handle1 = executor.handle();
        let handle2 = executor.handle();
        let counter = Arc::new(Mutex::new(0));

        let counter_clone1 = Arc::clone(&counter);
        let t1 = thread::spawn(move || {
            for _ in 0..5 {
                let counter = Arc::clone(&counter_clone1);
                handle1.execute(move || {
                    let mut num = counter.lock().unwrap();
                    *num += 1;
                });
            }
        });

        let counter_clone2 = Arc::clone(&counter);
        let t2 = thread::spawn(move || {
            for _ in 0..5 {
                let counter = Arc::clone(&counter_clone2);
                handle2.execute(move || {
                    let mut num = counter.lock().unwrap();
                    *num += 1;
                });
            }
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Give tasks time to complete
        thread::sleep(Duration::from_millis(100));

        assert_eq!(*counter.lock().unwrap(), 10);
    }

    #[test]
    fn test_executor_graceful_shutdown() {
        let executor = Executor::new(2);
        let completed = Arc::new(Mutex::new(false));

        let completed_clone = Arc::clone(&completed);
        executor.execute(move || {
            thread::sleep(Duration::from_millis(50));
            let mut done = completed_clone.lock().unwrap();
            *done = true;
        });

        // Drop executor to trigger shutdown
        drop(executor);

        // Task should have completed before shutdown
        assert!(*completed.lock().unwrap());
    }

    #[test]
    fn test_spawn_and_wait() {
        let executor = Executor::new(2);

        let future = executor.spawn(|| {
            thread::sleep(Duration::from_millis(50));
            42
        });

        let result = future.wait().unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_spawn_multiple_tasks() {
        let executor = Executor::new(4);

        let futures: Vec<_> = (0..10).map(|i| executor.spawn(move || i * 2)).collect();

        let results: Vec<_> = futures.into_iter().map(|f| f.wait().unwrap()).collect();

        assert_eq!(results, vec![0, 2, 4, 6, 8, 10, 12, 14, 16, 18]);
    }

    #[test]
    fn test_spawn_from_handle() {
        let executor = Executor::new(2);
        let handle = executor.handle();

        let future = thread::spawn(move || handle.spawn(|| "Hello from another thread"))
            .join()
            .unwrap();

        assert_eq!(future.wait().unwrap(), "Hello from another thread");
    }

    #[test]
    fn test_try_wait() {
        let executor = Executor::new(1);

        let future = executor.spawn(|| {
            thread::sleep(Duration::from_millis(100));
            42
        });

        // Should not be ready immediately
        assert_eq!(future.try_wait().unwrap(), None);

        // Wait for completion
        thread::sleep(Duration::from_millis(150));

        // Should be ready now
        assert_eq!(future.try_wait().unwrap(), Some(42));
    }

    #[test]
    fn test_spawn_with_string_result() {
        let executor = Executor::new(2);

        let future = executor.spawn(|| String::from("Task completed successfully"));

        let result = future.wait().unwrap();
        assert_eq!(result, "Task completed successfully");
    }

    #[test]
    fn test_scope_with_borrowed_data() {
        let executor = Executor::new(4);
        let mut data = vec![1, 2, 3, 4, 5];

        executor.scope(|s| {
            for item in &mut data {
                s.spawn(move || {
                    *item *= 2;
                });
            }
        });

        // All tasks guaranteed to complete
        assert_eq!(data, vec![2, 4, 6, 8, 10]);
    }

    #[test]
    fn test_scope_with_shared_reference() {
        let executor = Executor::new(2);
        let shared_data = vec![1, 2, 3, 4, 5];
        let sum = Arc::new(Mutex::new(0));

        executor.scope(|s| {
            for &item in &shared_data {
                let sum = Arc::clone(&sum);
                s.spawn(move || {
                    let mut total = sum.lock().unwrap();
                    *total += item;
                });
            }
        });

        assert_eq!(*sum.lock().unwrap(), 15);
    }

    #[test]
    fn test_scope_with_result() {
        let executor = Executor::new(2);
        let base = 10;

        let result = executor.scope(|s| {
            let futures: Vec<_> = (0..5)
                .map(|i| s.spawn_with_result(move || base + i))
                .collect();

            futures.into_iter().map(|f| f.wait().unwrap()).sum::<i32>()
        });

        assert_eq!(result, 60); // 10+10+11+12+13+14 = 60
    }

    #[test]
    fn test_scope_with_immutable_borrow() {
        let executor = Executor::new(2);
        let data = vec![1, 2, 3, 4, 5];
        let sum = Arc::new(Mutex::new(0));

        executor.scope(|s| {
            for &item in &data {
                let sum = Arc::clone(&sum);
                s.spawn(move || {
                    *sum.lock().unwrap() += item;
                });
            }
        });

        assert_eq!(*sum.lock().unwrap(), 15);
    }

    #[test]
    fn test_scope_ensures_completion() {
        let executor = Executor::new(1);
        let completed = Arc::new(Mutex::new(vec![]));

        executor.scope(|s| {
            for i in 0..5 {
                let completed = Arc::clone(&completed);
                s.spawn(move || {
                    thread::sleep(Duration::from_millis(10));
                    completed.lock().unwrap().push(i);
                });
            }
        });

        // All tasks must have completed
        assert_eq!(completed.lock().unwrap().len(), 5);
    }
}
