#[path = "support/arguments_executable.rs"]
mod arguments_executable;
#[path = "support/change_directory_executable.rs"]
mod change_directory_executable;
#[path = "support/code_pages_executable.rs"]
mod code_pages_executable;
#[path = "support/command_line_executable.rs"]
mod command_line_executable;
#[path = "support/computer_name_executable.rs"]
mod computer_name_executable;
#[path = "support/cpinfo_executable.rs"]
mod cpinfo_executable;
#[path = "support/crt_executable.rs"]
mod crt_executable;
#[path = "support/crt_sort_cases.rs"]
mod crt_sort_cases;
#[path = "support/current_directory_executable.rs"]
mod current_directory_executable;
#[path = "support/cxx_exception_cases.rs"]
mod cxx_exception_cases;
#[path = "support/disk_geometry_cases.rs"]
mod disk_geometry_cases;
#[allow(dead_code)]
#[path = "support/dll_executable.rs"]
mod dll_executable;
#[path = "support/environment_executable.rs"]
mod environment_executable;
#[path = "support/error_mode_executable.rs"]
mod error_mode_executable;
#[path = "support/executable.rs"]
mod executable;
#[path = "support/file_attributes_executable.rs"]
mod file_attributes_executable;
#[path = "support/file_handle_cases.rs"]
mod file_handle_cases;
#[path = "support/file_read_cases.rs"]
mod file_read_cases;
#[path = "support/file_seek_cases.rs"]
mod file_seek_cases;
#[path = "support/file_size_cases.rs"]
mod file_size_cases;
#[path = "support/find_files_executable.rs"]
mod find_files_executable;
#[path = "support/fp_control_executable.rs"]
mod fp_control_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/initializer_executable.rs"]
mod initializer_executable;
#[path = "support/local_realloc_executable.rs"]
mod local_realloc_executable;
#[path = "support/millisecond_clock_executable.rs"]
mod millisecond_clock_executable;
#[path = "support/module_file_name_executable.rs"]
mod module_file_name_executable;
#[path = "support/modules_executable.rs"]
mod modules_executable;
#[path = "support/performance_clock_executable.rs"]
mod performance_clock_executable;
#[path = "support/process_version_executable.rs"]
mod process_version_executable;
#[path = "support/registry_default_executable.rs"]
mod registry_default_executable;
#[path = "support/registry_executable.rs"]
mod registry_executable;
#[path = "support/registry_value_executable.rs"]
mod registry_value_executable;
#[path = "support/resource_executable.rs"]
mod resource_executable;
#[path = "support/short_path_executable.rs"]
mod short_path_executable;
#[path = "support/startup_info_executable.rs"]
mod startup_info_executable;
#[path = "support/suspended_thread_executable.rs"]
mod suspended_thread_executable;
#[path = "support/thread_notifications_executable.rs"]
mod thread_notifications_executable;
#[path = "support/windows_format_executable.rs"]
mod windows_format_executable;
#[path = "support/windows_string_length_executable.rs"]
mod windows_string_length_executable;

#[path = "windows/windows_arguments.rs"]
mod windows_arguments;
#[path = "windows/windows_change_directory.rs"]
mod windows_change_directory;
#[path = "windows/windows_code_pages.rs"]
mod windows_code_pages;
#[path = "windows/windows_com_apartment.rs"]
mod windows_com_apartment;
#[path = "windows/windows_com_graph.rs"]
mod windows_com_graph;
#[path = "windows/windows_command_line.rs"]
mod windows_command_line;
#[path = "windows/windows_computer_name.rs"]
mod windows_computer_name;
#[path = "windows/windows_cpinfo.rs"]
mod windows_cpinfo;
#[path = "windows/windows_crt.rs"]
mod windows_crt;
#[path = "windows/windows_crt_sort.rs"]
mod windows_crt_sort;
#[path = "windows/windows_current_directory.rs"]
mod windows_current_directory;
#[path = "windows/windows_cxx_catch.rs"]
mod windows_cxx_catch;
#[path = "windows/windows_deferred_modules.rs"]
mod windows_deferred_modules;
#[path = "windows/windows_disk_geometry.rs"]
mod windows_disk_geometry;
#[path = "windows/windows_environment.rs"]
mod windows_environment;
#[path = "windows/windows_error_mode.rs"]
mod windows_error_mode;
#[path = "windows/windows_file_attributes.rs"]
mod windows_file_attributes;
#[path = "windows/windows_file_handles.rs"]
mod windows_file_handles;
#[path = "windows/windows_file_read.rs"]
mod windows_file_read;
#[path = "windows/windows_file_seek.rs"]
mod windows_file_seek;
#[path = "windows/windows_file_size.rs"]
mod windows_file_size;
#[path = "windows/windows_find_files.rs"]
mod windows_find_files;
#[path = "windows/windows_formatting.rs"]
mod windows_formatting;
#[path = "windows/windows_fp_control.rs"]
mod windows_fp_control;
#[path = "windows/windows_global_memory.rs"]
mod windows_global_memory;
#[path = "windows/windows_heap.rs"]
mod windows_heap;
#[path = "windows/windows_initializers.rs"]
mod windows_initializers;
#[path = "windows/windows_local_realloc.rs"]
mod windows_local_realloc;
#[path = "windows/windows_millisecond_clock.rs"]
mod windows_millisecond_clock;
#[path = "windows/windows_module_file_name.rs"]
mod windows_module_file_name;
#[path = "windows/windows_modules.rs"]
mod windows_modules;
#[path = "windows/windows_multibyte_character.rs"]
mod windows_multibyte_character;
#[path = "windows/windows_performance_clock.rs"]
mod windows_performance_clock;
#[path = "windows/windows_process.rs"]
mod windows_process;
#[path = "windows/windows_process_version.rs"]
mod windows_process_version;
#[path = "windows/windows_provider_errors.rs"]
mod windows_provider_errors;
#[path = "windows/windows_registry.rs"]
mod windows_registry;
#[path = "windows/windows_registry_defaults.rs"]
mod windows_registry_defaults;
#[path = "windows/windows_registry_values.rs"]
mod windows_registry_values;
#[path = "windows/windows_resources.rs"]
mod windows_resources;
#[path = "windows/windows_short_path.rs"]
mod windows_short_path;
#[path = "windows/windows_startup_info.rs"]
mod windows_startup_info;
#[path = "windows/windows_string_append.rs"]
mod windows_string_append;
#[path = "windows/windows_string_copy.rs"]
mod windows_string_copy;
#[path = "windows/windows_string_length.rs"]
mod windows_string_length;
#[path = "windows/windows_system_directory.rs"]
mod windows_system_directory;
#[path = "windows/windows_terminated_copy.rs"]
mod windows_terminated_copy;
#[path = "windows/windows_version.rs"]
mod windows_version;
#[path = "windows/windows_wide_character.rs"]
mod windows_wide_character;
