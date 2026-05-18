  (func $__sarif_text_builder_new (result i64)
    (local $state i32)
    i32.const 16
    call $alloc
    local.tee $state
    i64.extend_i32_u
    local.get $state
    i32.const 0
    i32.store offset=0
    local.get $state
    i32.const 0
    i32.store offset=4
    local.get $state
    i32.const 0
    i32.store offset=8
  )
  (func $__sarif_text_builder_reserve
    (param $state i32) (param $needed i32) (result i32)
    (local $data_ptr i32) (local $len i32) (local $cap i32)
    (local $new_cap i32) (local $new_ptr i32) (local $i i32)
    local.get $state
    i32.load offset=0
    local.set $data_ptr
    local.get $state
    i32.load offset=4
    local.set $len
    local.get $state
    i32.load offset=8
    local.set $cap
    local.get $len
    local.get $needed
    i32.add
    local.tee $needed
    local.get $cap
    i32.le_u
    if
      local.get $state
      return
    end
    local.get $cap
    i32.eqz
    if
      i32.const 128
      local.set $new_cap
      local.get $needed
      i32.const 128
      i32.gt_u
      if
        local.get $needed
        local.set $new_cap
      end
    else
      local.get $cap
      i32.const 1
      i32.shl
      local.tee $new_cap
      local.get $needed
      i32.lt_u
      if
        local.get $needed
        local.set $new_cap
      end
    end
    local.get $new_cap
    call $alloc
    local.set $new_ptr
    i32.const 0
    local.set $i
    block $copy_done
      loop $copy
        local.get $i
        local.get $len
        i32.ge_u
        br_if $copy_done
        local.get $new_ptr
        local.get $i
        i32.add
        local.get $data_ptr
        local.get $i
        i32.add
        i32.load8_u
        i32.store8
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $copy
      end
    end
    local.get $state
    local.get $new_ptr
    i32.store offset=0
    local.get $state
    local.get $new_cap
    i32.store offset=8
    local.get $state
  )
  (func $__sarif_text_builder_append
    (param $builder i64) (param $text i64) (result i64)
    (local $state i32) (local $text_ptr i32) (local $text_len i32) (local $data_ptr i32) (local $len i32) (local $i i32)
    local.get $builder
    i32.wrap_i64
    local.set $state
    local.get $text
    i32.wrap_i64
    local.set $text_ptr
    local.get $text
    call $__sarif_text_len_i32
    local.tee $text_len
    i32.eqz
    if
      local.get $builder
      return
    end
    local.get $state
    local.get $text_len
    call $__sarif_text_builder_reserve
    drop
    local.get $state
    i32.load offset=0
    local.set $data_ptr
    local.get $state
    i32.load offset=4
    local.set $len
    i32.const 0
    local.set $i
    block $append_done
      loop $append
        local.get $i
        local.get $text_len
        i32.ge_u
        br_if $append_done
        local.get $data_ptr
        local.get $len
        i32.add
        local.get $i
        i32.add
        local.get $text_ptr
        local.get $i
        i32.add
        i32.load8_u
        i32.store8
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $append
      end
    end
    local.get $state
    local.get $len
    local.get $text_len
    i32.add
    i32.store offset=4
    local.get $builder
  )
  (func $__sarif_text_builder_append_codepoint
    (param $builder i64) (param $codepoint i64) (result i64)
    (local $state i32) (local $cp i32) (local $encoded i32) (local $data_ptr i32) (local $len i32)
    local.get $builder
    i32.wrap_i64
    local.set $state
    local.get $codepoint
    i32.wrap_i64
    local.tee $cp
    i32.const 0
    i32.lt_s
    if
      unreachable
    end
    local.get $cp
    i32.const 0x10ffff
    i32.gt_u
    if
      unreachable
    end
    local.get $cp
    i32.const 0xd800
    i32.ge_u
    local.get $cp
    i32.const 0xdfff
    i32.le_u
    i32.and
    if
      unreachable
    end
    local.get $cp
    i32.const 0x7f
    i32.le_u
    if
      local.get $state
      i32.const 1
      call $__sarif_text_builder_reserve
      drop
      local.get $state
      i32.load offset=0
      local.set $data_ptr
      local.get $state
      i32.load offset=4
      local.set $len
      local.get $data_ptr
      local.get $len
      i32.add
      local.get $cp
      i32.store8
      local.get $state
      local.get $len
      i32.const 1
      i32.add
      i32.store offset=4
      local.get $builder
      return
    end
    local.get $cp
    i32.const 0x7ff
    i32.le_u
    if
      local.get $state
      i32.const 2
      call $__sarif_text_builder_reserve
      drop
      local.get $state
      i32.load offset=0
      local.set $data_ptr
      local.get $state
      i32.load offset=4
      local.set $len
      local.get $data_ptr
      local.get $len
      i32.add
      i32.const 0xc0
      local.get $cp
      i32.const 6
      i32.shr_u
      i32.or
      i32.store8
      local.get $data_ptr
      local.get $len
      i32.add
      i32.const 1
      i32.add
      i32.const 0x80
      local.get $cp
      i32.const 0x3f
      i32.and
      i32.or
      i32.store8
      local.get $state
      local.get $len
      i32.const 2
      i32.add
      i32.store offset=4
      local.get $builder
      return
    end
    local.get $cp
    i32.const 0xffff
    i32.le_u
    if
      local.get $state
      i32.const 3
      call $__sarif_text_builder_reserve
      drop
      local.get $state
      i32.load offset=0
      local.set $data_ptr
      local.get $state
      i32.load offset=4
      local.set $len
      local.get $data_ptr
      local.get $len
      i32.add
      i32.const 0xe0
      local.get $cp
      i32.const 12
      i32.shr_u
      i32.or
      i32.store8
      local.get $data_ptr
      local.get $len
      i32.add
      i32.const 1
      i32.add
      i32.const 0x80
      local.get $cp
      i32.const 6
      i32.shr_u
      i32.const 0x3f
      i32.and
      i32.or
      i32.store8
      local.get $data_ptr
      local.get $len
      i32.add
      i32.const 2
      i32.add
      i32.const 0x80
      local.get $cp
      i32.const 0x3f
      i32.and
      i32.or
      i32.store8
      local.get $state
      local.get $len
      i32.const 3
      i32.add
      i32.store offset=4
      local.get $builder
      return
    end
    local.get $state
    i32.const 4
    call $__sarif_text_builder_reserve
    drop
    local.get $state
    i32.load offset=0
    local.set $data_ptr
    local.get $state
    i32.load offset=4
    local.set $len
    local.get $data_ptr
    local.get $len
    i32.add
    i32.const 0xf0
    local.get $cp
    i32.const 18
    i32.shr_u
    i32.or
    i32.store8
    local.get $data_ptr
    local.get $len
    i32.add
    i32.const 1
    i32.add
    i32.const 0x80
    local.get $cp
    i32.const 12
    i32.shr_u
    i32.const 0x3f
    i32.and
    i32.or
    i32.store8
    local.get $data_ptr
    local.get $len
    i32.add
    i32.const 2
    i32.add
    i32.const 0x80
    local.get $cp
    i32.const 6
    i32.shr_u
    i32.const 0x3f
    i32.and
    i32.or
    i32.store8
    local.get $data_ptr
    local.get $len
    i32.add
    i32.const 3
    i32.add
    i32.const 0x80
    local.get $cp
    i32.const 0x3f
    i32.and
    i32.or
    i32.store8
    local.get $state
    local.get $len
    i32.const 4
    i32.add
    i32.store offset=4
    local.get $builder
  )
  (func $__sarif_text_builder_append_ascii
    (param $builder i64) (param $byte i64) (result i64)
    (local $state i32) (local $b i32) (local $data_ptr i32) (local $len i32)
    local.get $builder
    i32.wrap_i64
    local.set $state
    local.get $byte
    i32.wrap_i64
    local.tee $b
    i32.const 0
    i32.lt_s
    if
      unreachable
    end
    local.get $b
    i32.const 0x7f
    i32.gt_u
    if
      unreachable
    end
    local.get $state
    i32.const 1
    call $__sarif_text_builder_reserve
    drop
    local.get $state
    i32.load offset=0
    local.set $data_ptr
    local.get $state
    i32.load offset=4
    local.set $len
    local.get $data_ptr
    local.get $len
    i32.add
    local.get $b
    i32.store8
    local.get $state
    local.get $len
    i32.const 1
    i32.add
    i32.store offset=4
    local.get $builder
  )
  (func $__sarif_text_builder_append_slice
    (param $builder i64) (param $text i64) (param $start i64) (param $end i64) (result i64)
    (local $state i32) (local $text_ptr i32) (local $text_len i32)
    (local $s i32) (local $e i32) (local $slice_len i32)
    (local $data_ptr i32) (local $len i32) (local $i i32)
    local.get $builder
    i32.wrap_i64
    local.set $state
    local.get $text
    i32.wrap_i64
    local.set $text_ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $text_len
    local.get $start
    i32.wrap_i64
    local.tee $s
    i32.const 0
    i32.lt_s
    if
      unreachable
    end
    local.get $end
    i32.wrap_i64
    local.tee $e
    local.get $s
    i32.lt_s
    if
      unreachable
    end
    local.get $e
    local.get $text_len
    i32.gt_u
    if
      unreachable
    end
    local.get $e
    local.get $s
    i32.sub
    local.tee $slice_len
    i32.eqz
    if
      local.get $builder
      return
    end
    local.get $state
    local.get $slice_len
    call $__sarif_text_builder_reserve
    drop
    local.get $state
    i32.load offset=0
    local.set $data_ptr
    local.get $state
    i32.load offset=4
    local.set $len
    i32.const 0
    local.set $i
    block $append_done
      loop $append
        local.get $i
        local.get $slice_len
        i32.ge_u
        br_if $append_done
        local.get $data_ptr
        local.get $len
        i32.add
        local.get $i
        i32.add
        local.get $text_ptr
        local.get $s
        i32.add
        local.get $i
        i32.add
        i32.load8_u
        i32.store8
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $append
      end
    end
    local.get $state
    local.get $len
    local.get $slice_len
    i32.add
    i32.store offset=4
    local.get $builder
  )
  (func $__sarif_text_builder_append_i32
    (param $builder i64) (param $value i64) (result i64)
    (local $state i32) (local $buf i32) (local $i i32) (local $neg i32)
    (local $mag i64) (local $formatted i64)
    local.get $builder
    i32.wrap_i64
    local.set $state
    i32.const 21
    call $alloc
    local.set $buf
    i32.const 20
    local.set $i
    local.get $value
    i64.const 0
    i64.lt_s
    if
      i32.const 1
      local.set $neg
      local.get $buf
      i32.const 20
      i32.add
      i64.const 0
      local.get $value
      i64.const 10
      i64.rem_s
      i64.sub
      i32.wrap_i64
      i32.const 48
      i32.add
      i32.store8
      i64.const 0
      local.get $value
      i64.const 10
      i64.div_s
      i64.sub
      local.set $mag
      i32.const 19
      local.set $i
    else
      i32.const 0
      local.set $neg
      local.get $value
      local.set $mag
    end
    block $digits_done
      loop $digits
        local.get $mag
        i64.eqz
        br_if $digits_done
        local.get $buf
        local.get $i
        i32.add
        local.get $mag
        i64.const 10
        i64.rem_u
        i32.wrap_i64
        i32.const 48
        i32.add
        i32.store8
        local.get $mag
        i64.const 10
        i64.div_u
        local.set $mag
        local.get $i
        i32.const 1
        i32.sub
        local.set $i
        br $digits
      end
    end
    local.get $neg
    if
      local.get $buf
      local.get $i
      i32.add
      i32.const 45
      i32.store8
      local.get $builder
      local.get $buf
      local.get $i
      i32.add
      i32.const 21
      local.get $i
      i32.sub
      call $__sarif_pack_text
      call $__sarif_text_builder_append
      return
    end
    local.get $builder
    local.get $buf
    local.get $i
    i32.add
    i32.const 1
    i32.add
    i32.const 20
    local.get $i
    i32.sub
    call $__sarif_pack_text
    call $__sarif_text_builder_append
  )
  (func $__sarif_text_builder_finish (param $builder i64) (result i64)
    (local $state i32) (local $data_ptr i32) (local $len i32)
    local.get $builder
    i32.wrap_i64
    local.set $state
    local.get $state
    i32.load offset=0
    local.set $data_ptr
    local.get $state
    i32.load offset=4
    local.set $len
    local.get $data_ptr
    local.get $len
    call $__sarif_pack_text
  )
  (func $__sarif_stdout_write (param $text i64)
    (local $ptr i32) (local $len i32) (local $iovec i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.tee $len
    i32.eqz
    if
      return
    end
    i32.const 8
    call $alloc
    local.tee $iovec
    local.get $ptr
    i32.store offset=0
    local.get $iovec
    local.get $len
    i32.store offset=4
    i32.const 1
    local.get $iovec
    i32.const 1
    i32.const 0
    call $__wasi_fd_write
    drop
  )
  (func $__sarif_stdout_write_builder (param $builder i64) (result i64)
    (local $state i32) (local $data_ptr i32) (local $len i32) (local $iovec i32)
    local.get $builder
    i32.wrap_i64
    local.set $state
    local.get $state
    i32.load offset=0
    local.set $data_ptr
    local.get $state
    i32.load offset=4
    local.set $len
    local.get $len
    i32.eqz
    if
      local.get $builder
      return
    end
    i32.const 8
    call $alloc
    local.tee $iovec
    local.get $data_ptr
    i32.store offset=0
    local.get $iovec
    local.get $len
    i32.store offset=4
    i32.const 1
    local.get $iovec
    i32.const 1
    i32.const 0
    call $__wasi_fd_write
    drop
    local.get $state
    i32.const 0
    i32.store offset=4
    local.get $builder
  )
  (func $__sarif_text_hash (param $text i64) (result i32)
    (local $ptr i32) (local $len i32) (local $hash i32) (local $i i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    i32.const -2128831035
    local.set $hash
    block $hash_done
      i32.const 0
      local.set $i
      loop $hash_loop
        local.get $i
        local.get $len
        i32.ge_u
        br_if $hash_done
        local.get $hash
        local.get $ptr
        local.get $i
        i32.add
        i32.load8_u
        i32.xor
        i32.const 16777619
        i32.mul
        local.set $hash
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $hash_loop
      end
    end
    local.get $hash
    local.get $len
    i32.xor
    local.tee $hash
    i32.eqz
    if
      i32.const 1
      return
    end
    local.get $hash
  )
  (func $__sarif_text_index_new (result i64)
    (local $index i32) (local $cap i32) (local $entries i32) (local $i i32)
    i32.const 12
    call $alloc
    local.tee $index
    i32.const 0
    i32.store offset=0
    local.get $index
    i32.const 8
    i32.store offset=4
    i32.const 8
    i32.const 24
    i32.mul
    call $alloc
    local.set $entries
    local.get $index
    local.get $entries
    i32.store offset=8
    i32.const 8
    local.set $cap
    block $zero_done
      i32.const 0
      local.set $i
      loop $zero
        local.get $i
        local.get $cap
        i32.ge_u
        br_if $zero_done
        local.get $entries
        local.get $i
        i32.const 24
        i32.mul
        i32.add
        i64.const 0
        i64.store offset=0
        local.get $entries
        local.get $i
        i32.const 24
        i32.mul
        i32.add
        i64.const 0
        i64.store offset=8
        local.get $entries
        local.get $i
        i32.const 24
        i32.mul
        i32.add
        i32.const 0
        i32.store offset=16
        local.get $entries
        local.get $i
        i32.const 24
        i32.mul
        i32.add
        i32.const 0
        i32.store offset=20
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $zero
      end
    end
    local.get $index
    i64.extend_i32_u
  )
  (func $__sarif_text_index_ensure_capacity (param $index i32) (result i32)
    (local $len i32) (local $cap i32) (local $entries i32) (local $new_cap i32)
    (local $new_entries i32) (local $i i32) (local $idx i32) (local $hash i32)
    local.get $index
    i32.load offset=0
    local.set $len
    local.get $index
    i32.load offset=4
    local.tee $cap
    local.set $new_cap
    local.get $index
    i32.load offset=8
    local.set $entries
    local.get $len
    i32.const 4
    i32.mul
    local.get $cap
    i32.const 3
    i32.mul
    i32.lt_u
    if
      i32.const 1
      return
    end
    local.get $cap
    i32.const 2
    i32.mul
    local.tee $new_cap
    i32.const 24
    i32.mul
    call $alloc
    local.set $new_entries
    block $rehash_done
      i32.const 0
      local.set $i
      loop $rehash
        local.get $i
        local.get $cap
        i32.ge_u
        br_if $rehash_done
        local.get $entries
        local.get $i
        i32.const 24
        i32.mul
        i32.add
        local.tee $idx
        i32.load offset=20
        i32.eqz
        if
          local.get $i
          i32.const 1
          i32.add
          local.set $i
          br $rehash
        end
        local.get $idx
        i32.load offset=16
        local.set $hash
        local.get $hash
        local.get $new_cap
        i32.rem_u
        local.set $idx
        block $probe_done
          loop $probe
            local.get $new_entries
            local.get $idx
            i32.const 24
            i32.mul
            i32.add
            i32.load offset=20
            i32.eqz
            br_if $probe_done
            local.get $idx
            i32.const 1
            i32.add
            local.get $new_cap
            i32.rem_u
            local.set $idx
            br $probe
          end
        end
        local.get $new_entries
        local.get $idx
        i32.const 24
        i32.mul
        i32.add
        local.get $entries
        local.get $i
        i32.const 24
        i32.mul
        i32.add
        i64.load offset=0
        i64.store offset=0
        local.get $new_entries
        local.get $idx
        i32.const 24
        i32.mul
        i32.add
        local.get $entries
        local.get $i
        i32.const 24
        i32.mul
        i32.add
        i64.load offset=8
        i64.store offset=8
        local.get $new_entries
        local.get $idx
        i32.const 24
        i32.mul
        i32.add
        local.get $hash
        i32.store offset=16
        local.get $new_entries
        local.get $idx
        i32.const 24
        i32.mul
        i32.add
        i32.const 1
        i32.store offset=20
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $rehash
      end
    end
    local.get $index
    local.get $new_entries
    i32.store offset=8
    local.get $index
    local.get $new_cap
    i32.store offset=4
    i32.const 1
  )
  (func $__sarif_text_index_find_entry
    (param $index i32) (param $key i64) (param $hash i32) (result i32)
    (local $cap i32) (local $entries i32) (local $idx i32) (local $start i32)
    local.get $index
    i32.load offset=4
    local.set $cap
    local.get $index
    i32.load offset=8
    local.set $entries
    local.get $hash
    local.get $cap
    i32.rem_u
    local.tee $idx
    local.set $start
    block $find_done
      loop $find
        local.get $entries
        local.get $idx
        i32.const 24
        i32.mul
        i32.add
        i32.load offset=20
        i32.eqz
        br_if $find_done
        local.get $entries
        local.get $idx
        i32.const 24
        i32.mul
        i32.add
        i32.load offset=16
        local.get $hash
        i32.ne
        if
          local.get $idx
          i32.const 1
          i32.add
          local.get $cap
          i32.rem_u
          local.set $idx
          local.get $idx
          local.get $start
          i32.ne
          br_if $find
          br $find_done
        end
        local.get $entries
        local.get $idx
        i32.const 24
        i32.mul
        i32.add
        i64.load offset=0
        local.get $key
        call $__sarif_text_eq
        i32.wrap_i64
        if
          local.get $entries
          local.get $idx
          i32.const 24
          i32.mul
          i32.add
          return
        end
        local.get $idx
        i32.const 1
        i32.add
        local.get $cap
        i32.rem_u
        local.set $idx
        local.get $idx
        local.get $start
        i32.ne
        br_if $find
      end
    end
    local.get $entries
    local.get $idx
    i32.const 24
    i32.mul
    i32.add
  )
  (func $__sarif_text_index_get
    (param $index i64) (param $key i64) (result i64)
    (local $entry i32)
    local.get $index
    i32.wrap_i64
    local.get $key
    local.get $key
    call $__sarif_text_hash
    call $__sarif_text_index_find_entry
    local.tee $entry
    i32.load offset=20
    if
      local.get $entry
      i64.load offset=8
      return
    end
    i64.const -1
  )
  (func $__sarif_text_index_contains
    (param $index i64) (param $key i64) (result i32)
    (local $entry i32)
    local.get $index
    i32.wrap_i64
    local.get $key
    local.get $key
    call $__sarif_text_hash
    call $__sarif_text_index_find_entry
    local.tee $entry
    i32.load offset=20
  )
  (func $__sarif_text_index_set
    (param $index i64) (param $key i64) (param $value i64) (result i64)
    (local $ptr i32) (local $hash i32) (local $entry i32)
    local.get $index
    i32.wrap_i64
    local.tee $ptr
    call $__sarif_text_index_ensure_capacity
    i32.eqz
    if
      i64.const -1
      return
    end
    local.get $key
    call $__sarif_text_hash
    local.set $hash
    local.get $ptr
    local.get $key
    local.get $hash
    call $__sarif_text_index_find_entry
    local.tee $entry
    i32.load offset=20
    if
      local.get $entry
      local.get $value
      i64.store offset=8
      local.get $index
      return
    end
    local.get $entry
    local.get $key
    i64.store offset=0
    local.get $entry
    local.get $value
    i64.store offset=8
    local.get $entry
    local.get $hash
    i32.store offset=16
    local.get $entry
    i32.const 1
    i32.store offset=20
    local.get $ptr
    local.get $ptr
    i32.load offset=0
    i32.const 1
    i32.add
    i32.store offset=0
    local.get $index
  )
  (func $__sarif_text_index_get_or_insert
    (param $index i64) (param $key i64) (param $default i64) (result i64)
    (local $ptr i32) (local $hash i32) (local $entry i32)
    local.get $index
    i32.wrap_i64
    local.tee $ptr
    call $__sarif_text_index_ensure_capacity
    i32.eqz
    if
      i64.const -1
      return
    end
    local.get $key
    call $__sarif_text_hash
    local.set $hash
    local.get $ptr
    local.get $key
    local.get $hash
    call $__sarif_text_index_find_entry
    local.tee $entry
    i32.load offset=20
    if
      local.get $entry
      i64.load offset=8
      return
    end
    local.get $entry
    local.get $key
    i64.store offset=0
    local.get $entry
    local.get $default
    i64.store offset=8
    local.get $entry
    local.get $hash
    i32.store offset=16
    local.get $entry
    i32.const 1
    i32.store offset=20
    local.get $ptr
    local.get $ptr
    i32.load offset=0
    i32.const 1
    i32.add
    i32.store offset=0
    local.get $default
  )
