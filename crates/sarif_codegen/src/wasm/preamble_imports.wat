(module
 (import "wasi_snapshot_preview1" "fd_write" (func $__wasi_fd_write (param i32 i32 i32 i32) (result i32)))
 (import "env" "__host_argc" (func $__host_argc (result i64)))
 (import "env" "__host_argv" (func $__host_argv (param $index i64) (param $buf_ptr i32) (param $buf_len i32) (result i32)))
 (import "env" "__host_stdin_read" (func $__host_stdin_read (param $buf_ptr i32) (param $buf_len i32) (result i32)))