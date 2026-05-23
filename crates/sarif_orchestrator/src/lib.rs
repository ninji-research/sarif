use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use sarif_codegen::{Program, RuntimeValue, run_function_wasm};

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
        self.status.values().all(|s| matches!(s, TaskStatus::Completed(_) | TaskStatus::Failed(_)))
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
                state.status.insert(dep.clone(), TaskStatus::Failed(err_msg.to_string()));
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
            TaskArg::Ref(dep_id) => {
                if let Some(TaskStatus::Completed(val)) = status.get(dep_id) {
                    val.clone()
                } else {
                    panic!("Dependency task '{dep_id}' was not completed successfully before dependent task was run");
                }
            }
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
        // 1. Try local queue (pop from front)
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

        // 2. Try stealing from other queues (pop from back to reduce contention)
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

        // 3. Try global queue or wait
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
            state.status.insert(task_id.to_string(), TaskStatus::Completed(val));
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
            state.status.insert(task_id.to_string(), TaskStatus::Failed(err.clone()));
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

/// Executes a DAG of tasks in parallel using a work-stealing DAG scheduler.
///
/// Each task calls a pure Sarif function in the provided `Program` using Wasmtime execution.
///
/// # Panics
///
/// Panics if joining any of the worker threads fails.
#[must_use]
pub fn execute_dag(
    program: &Program,
    tasks: Vec<Task>,
    num_threads: usize,
) -> HashMap<String, TaskStatus> {
    let mut status = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    let mut remaining_deps = HashMap::new();
    let mut tasks_map = HashMap::new();
    let mut global_queue = VecDeque::new();

    for task in &tasks {
        status.insert(task.id.clone(), TaskStatus::Pending);
        remaining_deps.insert(task.id.clone(), task.dependencies.len());

        for dep in &task.dependencies {
            dependents.entry(dep.clone()).or_default().push(task.id.clone());
        }

        if task.dependencies.is_empty() {
            global_queue.push_back(task.id.clone());
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
                let res = run_function_wasm(&program_clone, &task.function_name, &task.resolved_args)
                    .map_err(|e| e.message);
                complete_task(worker_id, &task.id, res, &shared_clone, &queues_clone);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let lock = shared.0.lock().unwrap();
    lock.status.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sarif_frontend::hir::lower as lower_hir;
    use sarif_syntax::ast::lower as lower_ast;
    use sarif_syntax::lexer::lex;
    use sarif_syntax::parser::parse;
    use sarif_codegen::lower;

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
}
