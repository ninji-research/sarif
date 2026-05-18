(module
  (import "wasi_snapshot_preview1" "fd_write" (func $__wasi_fd_write (param i32 i32 i32 i32) (result i32)))
  (import "env" "__host_argc" (func $__host_argc (result i64)))
  (import "env" "__host_argv" (func $__host_argv (param $index i64) (param $buf_ptr i32) (param $buf_len i32) (result i32)))
  (import "env" "__host_stdin_read" (func $__host_stdin_read (param $buf_ptr i32) (param $buf_len i32) (result i32)))
  (memory (export "memory") 1)
  (global $heap_ptr (mut i32) (i32.const 0))
  (func $alloc (param $size i32) (result i32) (local $ptr i32) (local $new_end i32) (local $pages i32)
    global.get $heap_ptr
    i32.const 7
    i32.add
    i32.const -8
    i32.and
    local.tee $ptr
    local.get $size
    i32.add
    local.tee $new_end
    memory.size
    i32.const 16
    i32.shl
    i32.gt_u
    if
      local.get $new_end
      memory.size
      i32.const 16
      i32.shl
      i32.sub
      i32.const 65535
      i32.add
      i32.const 16
      i32.shr_u
      local.set $pages
      local.get $pages
      memory.grow
      i32.const -1
      i32.eq
      if
        unreachable
      end
    end
    local.get $new_end
    global.set $heap_ptr
    local.get $ptr
  )
