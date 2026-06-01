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

 (func $__sarif_alloc_push
 global.get $alloc_stack_depth
 i32.const 256
 i32.ge_u
 if
 unreachable
 end
 global.get $alloc_stack_depth
 i32.const 4
 i32.mul
 global.get $heap_ptr
 i32.store
 global.get $alloc_stack_depth
 i32.const 1
 i32.add
 global.set $alloc_stack_depth
 )

 (func $__sarif_alloc_pop
 global.get $alloc_stack_depth
 i32.const 0
 i32.eq
 if
 return
 end
 global.get $alloc_stack_depth
 i32.const 1
 i32.sub
 global.set $alloc_stack_depth
 global.get $alloc_stack_depth
 i32.const 4
 i32.mul
 i32.load
 global.set $heap_ptr
 )