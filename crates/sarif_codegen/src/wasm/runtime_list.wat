  (func $__sarif_list_new (param $len i64) (param $fill i64) (result i64)
    (local $capacity i32) (local $header i32) (local $data i32) (local $i i32)
    (local $fill_lo i32) (local $fill_hi i32)
    local.get $len
    i32.wrap_i64
    local.set $capacity
    i32.const 8
    call $alloc
    local.tee $header
    local.get $capacity
    i32.store offset=0
    local.get $capacity
    i32.const 3
    i32.shl
    call $alloc
    local.tee $data
    local.get $header
    i32.store offset=4
    local.get $fill
    i32.wrap_i64
    local.set $fill_lo
    local.get $fill
    i64.const 32
    i64.shr_u
    i32.wrap_i64
    local.set $fill_hi
    i32.const 0
    local.set $i
    block $fill_done
      loop $fill_loop
        local.get $i
        local.get $capacity
        i32.ge_u
        br_if $fill_done
        local.get $data
        local.get $i
        i32.const 3
        i32.shl
        i32.add
        local.get $fill_lo
        i32.store offset=0
        local.get $data
        local.get $i
        i32.const 3
        i32.shl
        i32.add
        local.get $fill_hi
        i32.store offset=4
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $fill_loop
      end
    end
    local.get $header
    i64.extend_i32_u
  )
  (func $__sarif_list_len (param $list i64) (result i64)
    local.get $list
    i32.wrap_i64
    i32.load offset=0
    i64.extend_i32_u
  )
  (func $__sarif_list_get (param $list i64) (param $index i64) (result i64)
    (local $data i32) (local $idx i32) (local $result_lo i32) (local $result_hi i32)
    local.get $list
    i32.wrap_i64
    i32.load offset=4
    local.set $data
    local.get $index
    i32.wrap_i64
    local.tee $idx
    local.get $list
    i32.wrap_i64
    i32.load offset=0
    i32.ge_u
    if
      unreachable
    end
    local.get $data
    local.get $idx
    i32.const 3
    i32.shl
    i32.add
    local.tee $data
    i32.load offset=0
    local.set $result_lo
    local.get $data
    i32.load offset=4
    local.set $result_hi
    local.get $result_hi
    i64.extend_i32_u
    i64.const 32
    i64.shl
    local.get $result_lo
    i64.extend_i32_u
    i64.or
  )
  (func $__sarif_list_set (param $list i64) (param $index i64) (param $value i64) (result i64)
    (local $data i32) (local $idx i32)
    local.get $list
    i32.wrap_i64
    i32.load offset=4
    local.set $data
    local.get $index
    i32.wrap_i64
    local.tee $idx
    local.get $list
    i32.wrap_i64
    i32.load offset=0
    i32.ge_u
    if
      unreachable
    end
    local.get $data
    local.get $idx
    i32.const 3
    i32.shl
    i32.add
    local.get $value
    i32.wrap_i64
    i32.store offset=0
    local.get $data
    local.get $idx
    i32.const 3
    i32.shl
    i32.add
    local.get $value
    i64.const 32
    i64.shr_u
    i32.wrap_i64
    i32.store offset=4
    local.get $list
  )
  (func $__sarif_list_push (param $list i64) (param $used i64) (param $value i64) (result i64)
    (local $header i32) (local $cap i32) (local $u i32) (local $data i32)
    (local $new_cap i32) (local $new_data i32) (local $i i32)
    local.get $list
    i32.wrap_i64
    local.tee $header
    i32.load offset=0
    local.set $cap
    local.get $header
    i32.load offset=4
    local.set $data
    local.get $used
    i32.wrap_i64
    local.tee $u
    local.get $cap
    i32.lt_u
    if
      local.get $data
      local.get $u
      i32.const 3
      i32.shl
      i32.add
      local.get $value
      i32.wrap_i64
      i32.store offset=0
      local.get $data
      local.get $u
      i32.const 3
      i32.shl
      i32.add
      local.get $value
      i64.const 32
      i64.shr_u
      i32.wrap_i64
      i32.store offset=4
      local.get $list
      return
    end
    local.get $u
    local.get $cap
    i32.ne
    if
      unreachable
    end
    local.get $cap
    i32.eqz
    if
      i32.const 8
      local.set $new_cap
    else
      local.get $cap
      i32.const 1
      i32.shl
      local.set $new_cap
    end
    local.get $new_cap
    i32.const 3
    i32.shl
    call $alloc
    local.set $new_data
    i32.const 0
    local.set $i
    block $copy_done
      loop $copy
        local.get $i
        local.get $cap
        i32.ge_u
        br_if $copy_done
        local.get $new_data
        local.get $i
        i32.const 3
        i32.shl
        i32.add
        local.get $data
        local.get $i
        i32.const 3
        i32.shl
        i32.add
        i64.load
        i64.store
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $copy
      end
    end
    local.get $new_data
    local.get $u
    i32.const 3
    i32.shl
    i32.add
    local.get $value
    i32.wrap_i64
    i32.store offset=0
    local.get $new_data
    local.get $u
    i32.const 3
    i32.shl
    i32.add
    local.get $value
    i64.const 32
    i64.shr_u
    i32.wrap_i64
    i32.store offset=4
    local.get $header
    local.get $new_cap
    i32.store offset=0
    local.get $header
    local.get $new_data
    i32.store offset=4
    local.get $list
  )
  (func $__sarif_list_sort_text (param $list i64) (param $used i64) (result i64)
    (local $data i32) (local $u i32)
    (local $i i32) (local $j i32) (local $min_idx i32)
    (local $a i64) (local $b i64)
    local.get $list
    i32.wrap_i64
    i32.load offset=4
    local.set $data
    local.get $used
    i32.wrap_i64
    local.tee $u
    i32.const 2
    i32.lt_u
    if
      local.get $list
      return
    end
    i32.const 0
    local.set $i
    block $outer_done
      loop $outer
        local.get $i
        local.get $u
        i32.const 1
        i32.sub
        i32.ge_u
        br_if $outer_done
        local.get $i
        local.set $min_idx
        local.get $i
        i32.const 1
        i32.add
        local.set $j
        block $inner_done
          loop $inner
            local.get $j
            local.get $u
            i32.ge_u
            br_if $inner_done
            local.get $data
            local.get $j
            i32.const 3
            i32.shl
            i32.add
            i64.load
            local.set $b
            local.get $data
            local.get $min_idx
            i32.const 3
            i32.shl
            i32.add
            i64.load
            local.set $a
            local.get $a
            local.get $b
            call $__sarif_text_cmp
            i64.const 0
            i64.gt_s
            if
              local.get $j
              local.set $min_idx
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $inner
          end
        end
        local.get $min_idx
        local.get $i
        i32.ne
        if
          local.get $data
          local.get $i
          i32.const 3
          i32.shl
          i32.add
          i64.load
          local.set $a
          local.get $data
          local.get $min_idx
          i32.const 3
          i32.shl
          i32.add
          i64.load
          local.set $b
          local.get $data
          local.get $i
          i32.const 3
          i32.shl
          i32.add
          local.get $b
          i64.store
          local.get $data
          local.get $min_idx
          i32.const 3
          i32.shl
          i32.add
          local.get $a
          i64.store
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $outer
      end
    end
    local.get $list
  )
  (func $__sarif_list_sort_record_text_field (param $list i64) (param $used i64) (param $field_offset i64) (result i64)
    (local $data i32) (local $u i32) (local $fo i32)
    (local $i i32) (local $j i32) (local $min_idx i32)
    (local $a i64) (local $b i64)
    local.get $list
    i32.wrap_i64
    i32.load offset=4
    local.set $data
    local.get $used
    i32.wrap_i64
    local.tee $u
    i32.const 2
    i32.lt_u
    if
      local.get $list
      return
    end
    local.get $field_offset
    i32.wrap_i64
    local.set $fo
    i32.const 0
    local.set $i
    block $outer_done
      loop $outer
        local.get $i
        local.get $u
        i32.const 1
        i32.sub
        i32.ge_u
        br_if $outer_done
        local.get $i
        local.set $min_idx
        local.get $i
        i32.const 1
        i32.add
        local.set $j
        block $inner_done
          loop $inner
            local.get $j
            local.get $u
            i32.ge_u
            br_if $inner_done
            local.get $data
            local.get $min_idx
            i32.const 3
            i32.shl
            i32.add
            i64.load
            i32.wrap_i64
            local.get $fo
            i32.add
            i64.load
            local.set $a
            local.get $data
            local.get $j
            i32.const 3
            i32.shl
            i32.add
            i64.load
            i32.wrap_i64
            local.get $fo
            i32.add
            i64.load
            local.set $b
            local.get $a
            local.get $b
            call $__sarif_text_cmp
            i64.const 0
            i64.gt_s
            if
              local.get $j
              local.set $min_idx
            end
            local.get $j
            i32.const 1
            i32.add
            local.set $j
            br $inner
          end
        end
        local.get $min_idx
        local.get $i
        i32.ne
        if
          local.get $data
          local.get $i
          i32.const 3
          i32.shl
          i32.add
          i64.load
          local.set $a
          local.get $data
          local.get $min_idx
          i32.const 3
          i32.shl
          i32.add
          i64.load
          local.set $b
          local.get $data
          local.get $i
          i32.const 3
          i32.shl
          i32.add
          local.get $b
          i64.store
          local.get $data
          local.get $min_idx
          i32.const 3
          i32.shl
          i32.add
          local.get $a
          i64.store
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $outer
      end
    end
    local.get $list
  )
