use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::Hasher;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

/// Backend selection for task execution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuntimeBackend {
    #[default]
    Wasm,
    Native,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TaskArg {
    Const(RuntimeValue),
    Ref(String),
}

#[derive(Clone, Debug)]
pub struct Task {
    pub id: String,
    pub function_name: String,
    pub args: Vec<TaskArg>,
    pub dependencies: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed(RuntimeValue),
    Failed(String),
    Cached(RuntimeValue),
}

/// Cache entry for a single task execution.
#[derive(Clone, Debug)]
struct CacheEntry {
    output: RuntimeValue,
    input_hash: u64,
}

/// Cache of task execution results, keyed by task ID.
#[derive(Clone, Debug, Default)]
pub struct ExecutionCache {
    entries: HashMap<String, CacheEntry>,
}

impl ExecutionCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Check if a task has a valid cached result for the given inputs.
    /// Returns `Some(result)` if the cache entry matches, `None` otherwise.
    #[must_use]
    pub fn check(&self, task_id: &str, args: &[RuntimeValue]) -> Option<RuntimeValue> {
        let entry = self.entries.get(task_id)?;
        let hash = hash_args(args);
        if entry.input_hash == hash {
            Some(entry.output.clone())
        } else {
            None
        }
    }

    /// Store a result in the cache.
    pub fn store(&mut self, task_id: &str, args: &[RuntimeValue], result: &RuntimeValue) {
        self.entries.insert(
            task_id.to_string(),
            CacheEntry {
                output: result.clone(),
                input_hash: hash_args(args),
            },
        );
    }

    /// Invalidate a specific task's cached result and return dependent task IDs.
    pub fn invalidate(
        &mut self,
        task_id: &str,
        dependents: &HashMap<String, Vec<String>>,
    ) -> Vec<String> {
        self.entries.remove(task_id);
        let mut invalidated = vec![task_id.to_string()];
        if let Some(deps) = dependents.get(task_id) {
            for dep in deps.clone() {
                invalidated.extend(self.invalidate(&dep, dependents));
            }
        }
        invalidated
    }

    /// Invalidate all cached entries.
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }
}

// ---------------------------------------------------------------------------
// Shared state — unchanged from original except TaskStatus awareness
// ---------------------------------------------------------------------------

use sarif_codegen::{Program, RuntimeError, RuntimeValue};

#[cfg(feature = "backend-native")]
use sarif_codegen::run_function_native;

fn hash_args(args: &[RuntimeValue]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for arg in args {
        match arg {
            RuntimeValue::Int(v) => hasher.write_i64(*v),
            RuntimeValue::F64(v) => hasher.write_u64(v.to_bits()),
            RuntimeValue::Bool(v) => hasher.write_u64(u64::from(*v)),
            RuntimeValue::Text(v) => hasher.write(v.as_bytes()),
            RuntimeValue::Bytes(v) => hasher.write(v),
            RuntimeValue::Unit
            | RuntimeValue::Enum(_)
            | RuntimeValue::Record(_)
            | RuntimeValue::TextIndex(_)
            | RuntimeValue::TextBuilder(_)
            | RuntimeValue::File(_)
            | RuntimeValue::List(_) => hasher.write_u64(0),
        }
    }
    hasher.finish()
}

struct SharedState {
    tasks: HashMap<String, Task>,
    status: HashMap<String, TaskStatus>,
    dependents: HashMap<String, Vec<String>>,
    remaining_deps: HashMap<String, usize>,
    active_running: usize,
    global_queue: VecDeque<String>,
}

impl SharedState {
    fn is_finished(&self) -> bool {
        self.status.values().all(|s| {
            matches!(
                s,
                TaskStatus::Completed(_) | TaskStatus::Failed(_) | TaskStatus::Cached(_)
            )
        })
    }
}

struct WorkerQueue {
    queue: Mutex<VecDeque<String>>,
}

struct RunnableTask {
    id: String,
    function_name: String,
    resolved_args: Vec<RuntimeValue>,
}

fn fail_dependents_recursive(
    id: &str,
    err_msg: &str,
    state: &mut SharedState,
    new_completed_or_failed: &mut Vec<String>,
) {
    if let Some(deps) = state.dependents.get(id).cloned() {
        for dep in deps {
            let status = state.status.get(&dep);
            if status == Some(&TaskStatus::Pending) {
                state
                    .status
                    .insert(dep.clone(), TaskStatus::Failed(err_msg.to_string()));
                new_completed_or_failed.push(dep.clone());
                fail_dependents_recursive(&dep, err_msg, state, new_completed_or_failed);
            }
        }
    }
}

fn resolve_args(args: &[TaskArg], status: &HashMap<String, TaskStatus>) -> Vec<RuntimeValue> {
    args.iter()
        .map(|arg| match arg {
            TaskArg::Const(val) => val.clone(),
            TaskArg::Ref(dep_id) => match status.get(dep_id) {
                Some(TaskStatus::Completed(val) | TaskStatus::Cached(val)) => val.clone(),
                _ => {
                    panic!("Dependency task '{dep_id}' was not completed successfully before dependent task was run");
                }
            },
        })
        .collect()
}

fn get_next_task(
    worker_id: usize,
    shared: &Arc<(Mutex<SharedState>, Condvar)>,
    queues: &Arc<Vec<WorkerQueue>>,
) -> Option<RunnableTask> {
    let num_workers = queues.len();
    loop {
        if let Some(task_id) = {
            let mut q = queues[worker_id].queue.lock().unwrap();
            q.pop_front()
        } {
            let (lock, _) = &**shared;
            let mut state = lock.lock().unwrap();
            state.status.insert(task_id.clone(), TaskStatus::Running);
            state.active_running += 1;
            let task = state.tasks.get(&task_id).unwrap().clone();
            let resolved_args = resolve_args(&task.args, &state.status);
            drop(state);
            return Some(RunnableTask {
                id: task_id,
                function_name: task.function_name,
                resolved_args,
            });
        }

        for offset in 1..num_workers {
            let target_worker = (worker_id + offset) % num_workers;
            if let Some(task_id) = {
                let mut q = queues[target_worker].queue.lock().unwrap();
                q.pop_back()
            } {
                let (lock, _) = &**shared;
                let mut state = lock.lock().unwrap();
                state.status.insert(task_id.clone(), TaskStatus::Running);
                state.active_running += 1;
                let task = state.tasks.get(&task_id).unwrap().clone();
                let resolved_args = resolve_args(&task.args, &state.status);
                drop(state);
                return Some(RunnableTask {
                    id: task_id,
                    function_name: task.function_name,
                    resolved_args,
                });
            }
        }

        let (lock, cvar) = &**shared;
        let mut state = lock.lock().unwrap();

        if let Some(task_id) = state.global_queue.pop_front() {
            state.status.insert(task_id.clone(), TaskStatus::Running);
            state.active_running += 1;
            let task = state.tasks.get(&task_id).unwrap().clone();
            let resolved_args = resolve_args(&task.args, &state.status);
            drop(state);
            return Some(RunnableTask {
                id: task_id,
                function_name: task.function_name,
                resolved_args,
            });
        }

        if state.is_finished() {
            return None;
        }

        state = cvar.wait(state).unwrap();
        if state.is_finished() {
            return None;
        }
    }
}

fn complete_task(
    worker_id: usize,
    task_id: &str,
    result: Result<RuntimeValue, String>,
    shared: &Arc<(Mutex<SharedState>, Condvar)>,
    queues: &Arc<Vec<WorkerQueue>>,
) {
    let (lock, cvar) = &**shared;
    let mut state = lock.lock().unwrap();

    state.active_running -= 1;

    let mut new_ready_tasks = Vec::new();
    let mut new_completed_or_failed = Vec::new();

    match result {
        Ok(val) => {
            state
                .status
                .insert(task_id.to_string(), TaskStatus::Completed(val));
            new_completed_or_failed.push(task_id.to_string());

            if let Some(deps) = state.dependents.get(task_id).cloned() {
                for dep in deps {
                    if let Some(count) = state.remaining_deps.get_mut(&dep) {
                        *count -= 1;
                        if *count == 0 {
                            new_ready_tasks.push(dep);
                        }
                    }
                }
            }
        }
        Err(err) => {
            state
                .status
                .insert(task_id.to_string(), TaskStatus::Failed(err.clone()));
            new_completed_or_failed.push(task_id.to_string());

            fail_dependents_recursive(
                task_id,
                &format!("Dependency '{task_id}' failed: {err}"),
                &mut state,
                &mut new_completed_or_failed,
            );
        }
    }

    drop(state);

    if !new_ready_tasks.is_empty() {
        let mut local_q = queues[worker_id].queue.lock().unwrap();
        for ready in new_ready_tasks {
            local_q.push_front(ready);
        }
    }

    cvar.notify_all();
}

/// Execute a function using the selected backend.
fn execute_function(
    program: &Program,
    name: &str,
    args: &[RuntimeValue],
    backend: RuntimeBackend,
) -> Result<RuntimeValue, RuntimeError> {
    match backend {
        RuntimeBackend::Wasm => {
            #[cfg(feature = "backend-wasm")]
            {
                sarif_codegen::run_function_wasm(program, name, args)
                    .map_err(|e| RuntimeError::Message(e.message))
            }
            #[cfg(not(feature = "backend-wasm"))]
            {
                let _ = (program, name, args);
                Err(RuntimeError::Message(
                    "Wasm backend not available".to_string(),
                ))
            }
        }
        RuntimeBackend::Native => {
            #[cfg(feature = "backend-native")]
            {
                run_function_native(program, name, args)
            }
            #[cfg(not(feature = "backend-native"))]
            {
                let _ = (program, name, args);
                Err(RuntimeError::Message(
                    "Native backend not available".to_string(),
                ))
            }
        }
    }
}

/// Execute a DAG of tasks using the selected backend, with caching support.
///
/// If a `cache` is provided, task results are memoized. When re-executing
/// a task whose inputs haven't changed, the cached result is reused.
/// When inputs have changed, the stale entry is invalidated along with
/// all downstream dependents.
///
/// # Panics
///
/// Panics if joining any of the worker threads fails.
#[must_use]
pub fn execute_graph(
    program: &Program,
    tasks: Vec<Task>,
    backend: RuntimeBackend,
    cache: &mut ExecutionCache,
    num_threads: usize,
) -> HashMap<String, TaskStatus> {
    let mut status = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    let mut remaining_deps = HashMap::new();
    let mut tasks_map = HashMap::new();
    let mut global_queue = VecDeque::new();

    // Phase 1: Check cache and build initial ready set
    for task in &tasks {
        let deps_len = task.dependencies.len();
        remaining_deps.insert(task.id.clone(), deps_len);

        for dep in &task.dependencies {
            dependents
                .entry(dep.clone())
                .or_default()
                .push(task.id.clone());
        }

        if deps_len == 0 {
            // Leaf task — check cache immediately
            let resolved = resolve_args(&task.args, &status);
            let cached = cache.check(&task.id, &resolved);
            if let Some(val) = cached {
                status.insert(task.id.clone(), TaskStatus::Cached(val));
            } else {
                status.insert(task.id.clone(), TaskStatus::Pending);
                global_queue.push_back(task.id.clone());
            }
        } else {
            status.insert(task.id.clone(), TaskStatus::Pending);
        }
    }

    for task in tasks {
        tasks_map.insert(task.id.clone(), task);
    }

    let shared = Arc::new((
        Mutex::new(SharedState {
            tasks: tasks_map,
            status,
            dependents,
            remaining_deps,
            active_running: 0,
            global_queue,
        }),
        Condvar::new(),
    ));

    let queues = Arc::new(
        (0..num_threads)
            .map(|_| WorkerQueue {
                queue: Mutex::new(VecDeque::new()),
            })
            .collect::<Vec<_>>(),
    );

    let mut handles = Vec::new();

    for worker_id in 0..num_threads {
        let shared_clone = Arc::clone(&shared);
        let queues_clone = Arc::clone(&queues);
        let program_clone = program.clone();

        let handle = thread::spawn(move || {
            while let Some(task) = get_next_task(worker_id, &shared_clone, &queues_clone) {
                let res = execute_function(
                    &program_clone,
                    &task.function_name,
                    &task.resolved_args,
                    backend,
                )
                .map_err(|e| match e {
                    RuntimeError::Message(msg) => msg,
                    RuntimeError::EffectUnwind {
                        effect, operation, ..
                    } => format!("effect unwind: {effect}/{operation}"),
                });
                complete_task(worker_id, &task.id, res, &shared_clone, &queues_clone);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Phase 2: Populate cache from results
    let lock = shared.0.lock().unwrap();
    let final_status = lock.status.clone();
    // Clone task args for caching before dropping lock
    let cache_tasks: Vec<(String, Vec<TaskArg>)> = lock
        .tasks
        .iter()
        .map(|(id, task)| (id.clone(), task.args.clone()))
        .collect();
    drop(lock);

    for (task_id, args) in cache_tasks {
        let resolved = resolve_args(&args, &final_status);
        if let Some(TaskStatus::Completed(val)) = final_status.get(&task_id) {
            cache.store(&task_id, &resolved, val);
        }
    }

    final_status
}

/// Execute a DAG using the default Wasm backend (previously `execute_dag`).
///
/// This is a thin wrapper around [`execute_graph`] for backward compatibility.
#[must_use]
pub fn execute_dag(
    program: &Program,
    tasks: Vec<Task>,
    num_threads: usize,
) -> HashMap<String, TaskStatus> {
    let mut cache = ExecutionCache::new();
    execute_graph(
        program,
        tasks,
        RuntimeBackend::Wasm,
        &mut cache,
        num_threads,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sarif_codegen::lower;
    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;

    fn lower_program(source: &str) -> Program {
        let lexed = lex(source);
        let parsed = parse(&lexed.tokens);
        let ast = lower_ast(&parsed.root);
        let hir = lower_hir(&ast.file);
        lower(&hir.module).program
    }

    #[test]
    fn test_work_stealing_dag_execution() {
        let source = "
            fn add_one(x: I32) -> I32 {
                x + 1
            }
            fn double_val(x: I32) -> I32 {
                x * 2
            }
            fn sum_vals(x: I32, y: I32) -> I32 {
                x + y
            }
        ";
        let program = lower_program(source);

        let tasks = vec![
            Task {
                id: "t_add".to_string(),
                function_name: "add_one".to_string(),
                args: vec![TaskArg::Const(RuntimeValue::Int(10))],
                dependencies: vec![],
            },
            Task {
                id: "t_double".to_string(),
                function_name: "double_val".to_string(),
                args: vec![TaskArg::Const(RuntimeValue::Int(20))],
                dependencies: vec![],
            },
            Task {
                id: "t_sum".to_string(),
                function_name: "sum_vals".to_string(),
                args: vec![
                    TaskArg::Ref("t_add".to_string()),
                    TaskArg::Ref("t_double".to_string()),
                ],
                dependencies: vec!["t_add".to_string(), "t_double".to_string()],
            },
        ];

        let results = execute_dag(&program, tasks, 4);

        assert_eq!(
            results.get("t_add"),
            Some(&TaskStatus::Completed(RuntimeValue::Int(11)))
        );
        assert_eq!(
            results.get("t_double"),
            Some(&TaskStatus::Completed(RuntimeValue::Int(40)))
        );
        assert_eq!(
            results.get("t_sum"),
            Some(&TaskStatus::Completed(RuntimeValue::Int(51)))
        );
    }

    #[test]
    fn test_caching_reuse() {
        let source = "
            fn add_one(x: I32) -> I32 {
                x + 1
            }
        ";
        let program = lower_program(source);
        let mut cache = ExecutionCache::new();

        let tasks = vec![Task {
            id: "t1".to_string(),
            function_name: "add_one".to_string(),
            args: vec![TaskArg::Const(RuntimeValue::Int(10))],
            dependencies: vec![],
        }];

        // First run — executes
        let results = execute_graph(&program, tasks.clone(), RuntimeBackend::Wasm, &mut cache, 2);
        assert_eq!(
            results.get("t1"),
            Some(&TaskStatus::Completed(RuntimeValue::Int(11)))
        );
        assert!(cache.entries.contains_key("t1"));

        // Second run — should be cached (same inputs)
        let results2 = execute_graph(&program, tasks.clone(), RuntimeBackend::Wasm, &mut cache, 2);
        assert_eq!(
            results2.get("t1"),
            Some(&TaskStatus::Cached(RuntimeValue::Int(11)))
        );

        // Invalidate cache
        cache.invalidate_all();
        let results3 = execute_graph(&program, tasks, RuntimeBackend::Wasm, &mut cache, 2);
        assert_eq!(
            results3.get("t1"),
            Some(&TaskStatus::Completed(RuntimeValue::Int(11)))
        );
    }

    #[test]
    fn test_invalidation_propagation() {
        let source = "
            fn double_val(x: I32) -> I32 {
                x * 2
            }
        ";
        let program = lower_program(source);
        let mut cache = ExecutionCache::new();

        let mut dependents = HashMap::new();
        dependents.insert("t1".to_string(), vec!["t2".to_string(), "t3".to_string()]);

        // Store in cache
        cache.store("t1", &[RuntimeValue::Int(5)], &RuntimeValue::Int(10));

        // Invalidate t1 — should return t1, t2, t3
        let invalidated = cache.invalidate("t1", &dependents);
        assert!(invalidated.contains(&"t1".to_string()));
        assert!(invalidated.contains(&"t2".to_string()));
        assert!(invalidated.contains(&"t3".to_string()));
        assert!(!cache.entries.contains_key("t1"));
    }

    #[test]
    #[cfg(feature = "backend-native")]
    fn test_native_backend() {
        let source = "
            fn identity(x: I32) -> I32 {
                x
            }
        ";
        let program = lower_program(source);
        let mut cache = ExecutionCache::new();

        let tasks = vec![Task {
            id: "t1".to_string(),
            function_name: "identity".to_string(),
            args: vec![TaskArg::Const(RuntimeValue::Int(42))],
            dependencies: vec![],
        }];

        let results = execute_graph(&program, tasks, RuntimeBackend::Native, &mut cache, 2);
        assert_eq!(
            results.get("t1"),
            Some(&TaskStatus::Completed(RuntimeValue::Int(42)))
        );
    }
}
