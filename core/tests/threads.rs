#[path = "support/alternate_teb.rs"]
mod alternate_teb;
#[path = "support/counter_executable.rs"]
mod counter_executable;
#[allow(dead_code)]
#[path = "support/dll_executable.rs"]
mod dll_executable;
#[path = "support/event_executable.rs"]
mod event_executable;
#[path = "support/event_wait_cases.rs"]
mod event_wait_cases;
#[allow(dead_code)]
#[path = "support/event_wait_control.rs"]
mod event_wait_control;
#[path = "support/executable.rs"]
mod executable;
#[allow(dead_code)]
#[path = "support/hook_chain_cases.rs"]
mod hook_chain_cases;
#[path = "support/hook_executable.rs"]
mod hook_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/interlocked_executable.rs"]
mod interlocked_executable;
#[path = "support/mutex_executable.rs"]
mod mutex_executable;
#[path = "support/mutex_wait_cases.rs"]
mod mutex_wait_cases;
#[path = "support/resumed_thread_cases.rs"]
mod resumed_thread_cases;
#[path = "support/suspended_event_cases.rs"]
mod suspended_event_cases;
#[path = "support/suspended_thread_cases.rs"]
mod suspended_thread_cases;
#[path = "support/suspended_thread_executable.rs"]
mod suspended_thread_executable;
#[path = "support/thread_executable.rs"]
mod thread_executable;
#[path = "support/thread_hook_cases.rs"]
mod thread_hook_cases;
#[path = "support/thread_identity_executable.rs"]
mod thread_identity_executable;
#[path = "support/thread_priority_executable.rs"]
mod thread_priority_executable;
#[path = "support/thread_start_cases.rs"]
mod thread_start_cases;
#[path = "support/thread_window_query_cases.rs"]
mod thread_window_query_cases;
#[path = "support/timed_event_cases.rs"]
mod timed_event_cases;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

#[path = "threads/windows_critical_sections.rs"]
mod windows_critical_sections;
#[path = "threads/windows_event_waits.rs"]
mod windows_event_waits;
#[path = "threads/windows_events.rs"]
mod windows_events;
#[path = "threads/windows_hook_chain.rs"]
mod windows_hook_chain;
#[path = "threads/windows_hooks.rs"]
mod windows_hooks;
#[path = "threads/windows_interlocked.rs"]
mod windows_interlocked;
#[path = "threads/windows_interlocked_counters.rs"]
mod windows_interlocked_counters;
#[path = "threads/windows_mutex_waits.rs"]
mod windows_mutex_waits;
#[path = "threads/windows_mutexes.rs"]
mod windows_mutexes;
#[path = "threads/windows_resume_thread.rs"]
mod windows_resume_thread;
#[path = "threads/windows_suspend_thread.rs"]
mod windows_suspend_thread;
#[path = "threads/windows_thread.rs"]
mod windows_thread;
#[path = "threads/windows_thread_hooks.rs"]
mod windows_thread_hooks;
#[path = "threads/windows_thread_identity.rs"]
mod windows_thread_identity;
#[path = "threads/windows_thread_priority.rs"]
mod windows_thread_priority;
#[path = "threads/windows_thread_start.rs"]
mod windows_thread_start;
#[path = "threads/windows_thread_synchronization.rs"]
mod windows_thread_synchronization;
#[path = "threads/windows_thread_window_queries.rs"]
mod windows_thread_window_queries;
#[path = "threads/windows_threads.rs"]
mod windows_threads;
#[path = "threads/windows_timed_event_waits.rs"]
mod windows_timed_event_waits;
#[path = "threads/windows_tls.rs"]
mod windows_tls;
