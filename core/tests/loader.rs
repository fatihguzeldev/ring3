#[path = "support/buffer_compare_executable.rs"]
mod buffer_compare_executable;
#[path = "support/critical_section_executable.rs"]
mod critical_section_executable;
#[path = "support/crt_code_page_executable.rs"]
mod crt_code_page_executable;
#[path = "support/crt_heap_executable.rs"]
mod crt_heap_executable;
#[path = "support/delay_executable.rs"]
mod delay_executable;
#[path = "support/dll_executable.rs"]
mod dll_executable;
#[path = "support/dllonexit_executable.rs"]
mod dllonexit_executable;
#[path = "support/exception_frame_executable.rs"]
mod exception_frame_executable;
#[path = "support/executable.rs"]
mod executable;
#[path = "support/export_names_executable.rs"]
mod export_names_executable;
#[path = "support/global_memory_executable.rs"]
mod global_memory_executable;
#[path = "support/heap_executable.rs"]
mod heap_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/memset_executable.rs"]
mod memset_executable;
#[path = "support/on_demand_file_cases.rs"]
mod on_demand_file_cases;
#[path = "support/onexit_executable.rs"]
mod onexit_executable;
#[path = "support/performance_clock_executable.rs"]
mod performance_clock_executable;
#[path = "support/relocated_executable.rs"]
mod relocated_executable;
#[path = "support/resource_executable.rs"]
mod resource_executable;
#[path = "support/reverse_search_executable.rs"]
mod reverse_search_executable;
#[path = "support/strdup_executable.rs"]
mod strdup_executable;
#[path = "support/string_traversal_executable.rs"]
mod string_traversal_executable;
#[path = "support/system_directory_executable.rs"]
mod system_directory_executable;
#[path = "support/tls_executable.rs"]
mod tls_executable;

#[path = "loader/diagnostic_process.rs"]
mod diagnostic_process;
#[path = "loader/file_content_inputs.rs"]
mod file_content_inputs;
#[path = "loader/guest_buffer_compare.rs"]
mod guest_buffer_compare;
#[path = "loader/guest_critical_sections.rs"]
mod guest_critical_sections;
#[path = "loader/guest_crt_code_page.rs"]
mod guest_crt_code_page;
#[path = "loader/guest_crt_heap.rs"]
mod guest_crt_heap;
#[path = "loader/guest_delay_thunks.rs"]
mod guest_delay_thunks;
#[path = "loader/guest_dllonexit.rs"]
mod guest_dllonexit;
#[path = "loader/guest_dlls.rs"]
mod guest_dlls;
#[path = "loader/guest_exception_frame.rs"]
mod guest_exception_frame;
#[path = "loader/guest_global_memory.rs"]
mod guest_global_memory;
#[path = "loader/guest_heap.rs"]
mod guest_heap;
#[path = "loader/guest_memory.rs"]
mod guest_memory;
#[path = "loader/guest_memset.rs"]
mod guest_memset;
#[path = "loader/guest_onexit.rs"]
mod guest_onexit;
#[path = "loader/guest_relocations.rs"]
mod guest_relocations;
#[path = "loader/guest_resources.rs"]
mod guest_resources;
#[path = "loader/guest_reverse_search.rs"]
mod guest_reverse_search;
#[path = "loader/guest_strdup.rs"]
mod guest_strdup;
#[path = "loader/guest_string_traversal.rs"]
mod guest_string_traversal;
#[path = "loader/guest_system_directory.rs"]
mod guest_system_directory;
#[path = "loader/guest_tls.rs"]
mod guest_tls;
#[path = "loader/native_clock_driver.rs"]
mod native_clock_driver;
#[path = "loader/on_demand_file_contents.rs"]
mod on_demand_file_contents;
#[path = "loader/process_parameters.rs"]
mod process_parameters;
