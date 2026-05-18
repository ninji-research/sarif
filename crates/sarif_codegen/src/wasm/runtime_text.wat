  (func $__sarif_pack_text (param $ptr i32) (param $len i32) (result i64)
    local.get $ptr
    i64.extend_i32_u
    local.get $len
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.or
  )
  (func $__sarif_text_len_i32 (param $text i64) (result i32)
    local.get $text
    i64.const 32
    i64.shr_u
    i32.wrap_i64
  )
  (func $__sarif_is_ascii_space (param $byte i32) (result i32)
    local.get $byte
    i32.const 32
    i32.eq
    local.get $byte
    i32.const 10
    i32.eq
    i32.or
    local.get $byte
    i32.const 13
    i32.eq
    i32.or
    local.get $byte
    i32.const 9
    i32.eq
    i32.or
  )
  (func $__sarif_is_ascii_digit (param $byte i32) (result i32)
    local.get $byte
    i32.const 48
    i32.ge_u
    local.get $byte
    i32.const 57
    i32.le_u
    i32.and
  )
  (func $__sarif_is_utf8_continuation (param $byte i32) (result i32)
    local.get $byte
    i32.const 192
    i32.and
    i32.const 128
    i32.eq
  )
  (func $__sarif_text_eq (param $left i64) (param $right i64) (result i64)
    (local $left_ptr i32)
    (local $right_ptr i32)
    (local $left_len i32)
    (local $right_len i32)
    (local $index i32)
    (local $equal i64)
    local.get $left
    call $__sarif_text_len_i32
    local.set $left_len
    local.get $right
    call $__sarif_text_len_i32
    local.set $right_len
    local.get $left_len
    local.get $right_len
    i32.ne
    if
      i64.const 0
      return
    end
    local.get $left
    i32.wrap_i64
    local.set $left_ptr
    local.get $right
    i32.wrap_i64
    local.set $right_ptr
    i32.const 0
    local.set $index
    i64.const 1
    local.set $equal
    block $done
      loop $loop
        local.get $index
        local.get $left_len
        i32.ge_u
        br_if $done
        local.get $left_ptr
        local.get $index
        i32.add
        i32.load8_u
        local.get $right_ptr
        local.get $index
        i32.add
        i32.load8_u
        i32.ne
        if
          i64.const 0
          local.set $equal
          br $done
        end
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      end
    end
    local.get $equal
  )
  (func $__sarif_text_cmp (param $left i64) (param $right i64) (result i64)
    (local $left_ptr i32)
    (local $right_ptr i32)
    (local $left_len i32)
    (local $right_len i32)
    (local $limit i32)
    (local $index i32)
    (local $left_byte i32)
    (local $right_byte i32)
    local.get $left
    call $__sarif_text_len_i32
    local.set $left_len
    local.get $right
    call $__sarif_text_len_i32
    local.set $right_len
    local.get $left
    i32.wrap_i64
    local.set $left_ptr
    local.get $right
    i32.wrap_i64
    local.set $right_ptr
    local.get $left_len
    local.get $right_len
    i32.lt_u
    if (result i32)
      local.get $left_len
    else
      local.get $right_len
    end
    local.set $limit
    i32.const 0
    local.set $index
    block $done
      loop $loop
        local.get $index
        local.get $limit
        i32.ge_u
        br_if $done
        local.get $left_ptr
        local.get $index
        i32.add
        i32.load8_u
        local.tee $left_byte
        local.get $right_ptr
        local.get $index
        i32.add
        i32.load8_u
        local.tee $right_byte
        i32.ne
        if
          local.get $left_byte
          local.get $right_byte
          i32.lt_u
          if
            i64.const -1
            return
          end
          i64.const 1
          return
        end
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      end
    end
    local.get $left_len
    local.get $right_len
    i32.lt_u
    if
      i64.const -1
      return
    end
    local.get $left_len
    local.get $right_len
    i32.gt_u
    if
      i64.const 1
      return
    end
    i64.const 0
  )
  (func $__sarif_text_byte (param $text i64) (param $index i64) (result i64)
    (local $ptr i32)
    (local $len i32)
    (local $offset i32)
    local.get $index
    i64.const 0
    i64.lt_s
    if
      i64.const 0
      return
    end
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $index
    local.get $len
    i64.extend_i32_u
    i64.ge_u
    if
      i64.const 0
      return
    end
    local.get $index
    i32.wrap_i64
    local.set $offset
    local.get $ptr
    local.get $offset
    i32.add
    i32.load8_u
    i64.extend_i32_u
  )
  (func $__sarif_bytes_slice (param $bytes i64) (param $start_raw i64) (param $end_raw i64) (result i64)
    (local $ptr i32)
    (local $len i32)
    (local $start i32)
    (local $end i32)
    (local $dest_ptr i32)
    (local $dest_len i32)
    (local $index i32)
    local.get $bytes
    i32.wrap_i64
    local.set $ptr
    local.get $bytes
    call $__sarif_text_len_i32
    local.set $len
    local.get $start_raw
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $start
    else
      local.get $start_raw
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $start
      else
        local.get $start_raw
        i32.wrap_i64
        local.set $start
      end
    end
    local.get $end_raw
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $end
    else
      local.get $end_raw
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $end
      else
        local.get $end_raw
        i32.wrap_i64
        local.set $end
      end
    end
    local.get $end
    local.get $start
    i32.le_u
    if
      i64.const 0
      return
    end
    local.get $start
    i32.eqz
    local.get $end
    local.get $len
    i32.eq
    i32.and
    if
      local.get $bytes
      return
    end
    local.get $end
    local.get $start
    i32.sub
    local.tee $dest_len
    call $alloc
    local.set $dest_ptr
    i32.const 0
    local.set $index
    block $copy_done
      loop $copy
        local.get $index
        local.get $dest_len
        i32.ge_u
        br_if $copy_done
        local.get $dest_ptr
        local.get $index
        i32.add
        local.get $ptr
        local.get $start
        i32.add
        local.get $index
        i32.add
        i32.load8_u
        i32.store8
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $copy
      end
    end
    local.get $dest_ptr
    local.get $dest_len
    call $__sarif_pack_text
  )
  (func $__sarif_text_concat (param $left i64) (param $right i64) (result i64)
    (local $left_ptr i32)
    (local $right_ptr i32)
    (local $left_len i32)
    (local $right_len i32)
    (local $dest_ptr i32)
    (local $dest_len i32)
    (local $index i32)
    local.get $left
    call $__sarif_text_len_i32
    local.set $left_len
    local.get $right
    call $__sarif_text_len_i32
    local.set $right_len
    local.get $left_len
    i32.eqz
    if
      local.get $right
      return
    end
    local.get $right_len
    i32.eqz
    if
      local.get $left
      return
    end
    local.get $left
    i32.wrap_i64
    local.set $left_ptr
    local.get $right
    i32.wrap_i64
    local.set $right_ptr
    local.get $left_len
    local.get $right_len
    i32.add
    local.tee $dest_len
    call $alloc
    local.set $dest_ptr
    i32.const 0
    local.set $index
    block $copy_left_done
      loop $copy_left
        local.get $index
        local.get $left_len
        i32.ge_u
        br_if $copy_left_done
        local.get $dest_ptr
        local.get $index
        i32.add
        local.get $left_ptr
        local.get $index
        i32.add
        i32.load8_u
        i32.store8
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $copy_left
      end
    end
    i32.const 0
    local.set $index
    block $copy_right_done
      loop $copy_right
        local.get $index
        local.get $right_len
        i32.ge_u
        br_if $copy_right_done
        local.get $dest_ptr
        local.get $left_len
        i32.add
        local.get $index
        i32.add
        local.get $right_ptr
        local.get $index
        i32.add
        i32.load8_u
        i32.store8
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $copy_right
      end
    end
    local.get $dest_ptr
    local.get $dest_len
    call $__sarif_pack_text
  )
  (func $__sarif_clamp_text_slice_start (param $text i64) (param $index i64) (result i32)
    (local $ptr i32)
    (local $len i32)
    (local $result i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $index
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $result
    else
      local.get $index
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $result
      else
        local.get $index
        i32.wrap_i64
        local.set $result
      end
    end
    block $done
      loop $loop
        local.get $result
        local.get $len
        i32.ge_u
        br_if $done
        local.get $ptr
        local.get $result
        i32.add
        i32.load8_u
        call $__sarif_is_utf8_continuation
        i32.eqz
        br_if $done
        local.get $result
        i32.const 1
        i32.add
        local.set $result
        br $loop
      end
    end
    local.get $result
  )
  (func $__sarif_clamp_text_slice_end (param $text i64) (param $index i64) (result i32)
    (local $ptr i32)
    (local $len i32)
    (local $result i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $index
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $result
    else
      local.get $index
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $result
      else
        local.get $index
        i32.wrap_i64
        local.set $result
      end
    end
    block $done
      loop $loop
        local.get $result
        local.get $len
        i32.ge_u
        br_if $done
        local.get $ptr
        local.get $result
        i32.add
        i32.load8_u
        call $__sarif_is_utf8_continuation
        i32.eqz
        br_if $done
        local.get $result
        i32.const 1
        i32.sub
        local.set $result
        br $loop
      end
    end
    local.get $result
  )
  (func $__sarif_text_slice (param $text i64) (param $start_raw i64) (param $end_raw i64) (result i64)
    (local $ptr i32)
    (local $len i32)
    (local $start i32)
    (local $end i32)
    (local $dest_ptr i32)
    (local $dest_len i32)
    (local $index i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $text
    local.get $start_raw
    call $__sarif_clamp_text_slice_start
    local.set $start
    local.get $text
    local.get $end_raw
    call $__sarif_clamp_text_slice_end
    local.set $end
    local.get $end
    local.get $start
    i32.le_u
    if
      i64.const 0
      return
    end
    local.get $start
    i32.eqz
    local.get $end
    local.get $len
    i32.eq
    i32.and
    if
      local.get $text
      return
    end
    local.get $end
    local.get $start
    i32.sub
    local.tee $dest_len
    call $alloc
    local.set $dest_ptr
    i32.const 0
    local.set $index
    block $copy_done
      loop $copy
        local.get $index
        local.get $dest_len
        i32.ge_u
        br_if $copy_done
        local.get $dest_ptr
        local.get $index
        i32.add
        local.get $ptr
        local.get $start
        i32.add
        local.get $index
        i32.add
        i32.load8_u
        i32.store8
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $copy
      end
    end
    local.get $dest_ptr
    local.get $dest_len
    call $__sarif_pack_text
  )
  (func $__sarif_text_eq_range (param $text i64) (param $start i64) (param $end i64) (param $expected i64) (result i64)
    local.get $text
    local.get $start
    local.get $end
    call $__sarif_text_slice
    local.get $expected
    call $__sarif_text_eq
  )
  (func $__sarif_text_find_byte_range (param $text i64) (param $start_raw i64) (param $end_raw i64) (param $byte_raw i64) (result i64)
    (local $ptr i32)
    (local $len i32)
    (local $start i32)
    (local $end i32)
    (local $byte i32)
    (local $index i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    call $__sarif_text_len_i32
    local.set $len
    local.get $text
    local.get $start_raw
    call $__sarif_clamp_text_slice_start
    local.set $start
    local.get $text
    local.get $end_raw
    call $__sarif_clamp_text_slice_end
    local.set $end
    local.get $byte_raw
    i32.wrap_i64
    local.set $byte
    local.get $start
    local.set $index
    block $done
      loop $loop
        local.get $index
        local.get $end
        i32.ge_u
        br_if $done
        local.get $ptr
        local.get $index
        i32.add
        i32.load8_u
        local.get $byte
        i32.eq
        if
          local.get $index
          i64.extend_i32_u
          return
        end
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      end
    end
    local.get $end
    i64.extend_i32_u
  )
  (func $__sarif_bytes_find_byte_range (param $bytes i64) (param $start_raw i64) (param $end_raw i64) (param $byte_raw i64) (result i64)
    (local $ptr i32)
    (local $len i32)
    (local $start i32)
    (local $end i32)
    (local $byte i32)
    (local $index i32)
    local.get $bytes
    i32.wrap_i64
    local.set $ptr
    local.get $bytes
    call $__sarif_text_len_i32
    local.set $len
    local.get $start_raw
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $start
    else
      local.get $start_raw
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $start
      else
        local.get $start_raw
        i32.wrap_i64
        local.set $start
      end
    end
    local.get $end_raw
    i64.const 0
    i64.lt_s
    if
      i32.const 0
      local.set $end
    else
      local.get $end_raw
      local.get $len
      i64.extend_i32_u
      i64.gt_u
      if
        local.get $len
        local.set $end
      else
        local.get $end_raw
        i32.wrap_i64
        local.set $end
      end
    end
    local.get $byte_raw
    i32.wrap_i64
    local.set $byte
    local.get $start
    local.set $index
    block $done
      loop $loop
        local.get $index
        local.get $end
        i32.ge_u
        br_if $done
        local.get $ptr
        local.get $index
        i32.add
        i32.load8_u
        local.get $byte
        i32.eq
        if
          local.get $index
          i64.extend_i32_u
          return
        end
        local.get $index
        i32.const 1
        i32.add
        local.set $index
        br $loop
      end
    end
    local.get $end
    i64.extend_i32_u
  )
  (func $__sarif_text_line_end (param $text i64) (param $start i64) (result i64)
    (local $ptr i32)
    (local $line_end i64)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    local.get $text
    local.get $start
    local.get $text
    call $__sarif_text_len_i32
    i64.extend_i32_u
    i64.const 10
    call $__sarif_text_find_byte_range
    local.set $line_end
    local.get $line_end
    i64.const 0
    i64.gt_u
    if
      local.get $ptr
      local.get $line_end
      i32.wrap_i64
      i32.const 1
      i32.sub
      i32.add
      i32.load8_u
      i32.const 13
      i32.eq
      if
        local.get $line_end
        i64.const 1
        i64.sub
        return
      end
    end
    local.get $line_end
  )
  (func $__sarif_text_next_line (param $text i64) (param $start i64) (result i64)
    (local $len i64)
    (local $end i64)
    local.get $text
    call $__sarif_text_len_i32
    i64.extend_i32_u
    local.set $len
    local.get $text
    local.get $start
    call $__sarif_text_line_end
    local.set $end
    local.get $end
    local.get $len
    i64.lt_u
    if
      local.get $end
      i64.const 1
      i64.add
      return
    end
    local.get $end
  )
  (func $__sarif_text_field_end (param $text i64) (param $start i64) (param $end i64) (param $byte i64) (result i64)
    local.get $text
    local.get $start
    local.get $end
    local.get $byte
    call $__sarif_text_find_byte_range
  )
  (func $__sarif_text_next_field (param $text i64) (param $start i64) (param $end i64) (param $byte i64) (result i64)
    (local $field_end i64)
    local.get $text
    local.get $start
    local.get $end
    local.get $byte
    call $__sarif_text_field_end
    local.set $field_end
    local.get $field_end
    local.get $end
    i64.lt_u
    if
      local.get $field_end
      i64.const 1
      i64.add
      return
    end
    local.get $field_end
  )
  (func $__sarif_parse_i32 (param $text i64) (result i64)
    (local $ptr i32)
    (local $start i32)
    (local $end i32)
    (local $negative i32)
    (local $result i64)
    (local $byte i32)
    (local $has_digit i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    i32.const 0
    local.set $start
    local.get $text
    call $__sarif_text_len_i32
    local.set $end
    block $trim_start_done
      loop $trim_start
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_start_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_start_done
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $trim_start
      end
    end
    block $trim_end_done
      loop $trim_end
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_end_done
        local.get $ptr
        local.get $end
        i32.const 1
        i32.sub
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_end_done
        local.get $end
        i32.const 1
        i32.sub
        local.set $end
        br $trim_end
      end
    end
    local.get $start
    local.get $end
    i32.ge_u
    if
      unreachable
    end
    i32.const 0
    local.set $negative
    local.get $ptr
    local.get $start
    i32.add
    i32.load8_u
    local.tee $byte
    i32.const 45
    i32.eq
    if
      i32.const 1
      local.set $negative
      local.get $start
      i32.const 1
      i32.add
      local.set $start
    else
      local.get $byte
      i32.const 43
      i32.eq
      if
        local.get $start
        i32.const 1
        i32.add
        local.set $start
      end
    end
    i64.const 0
    local.set $result
    i32.const 0
    local.set $has_digit
    block $parse_done
      loop $parse
        local.get $start
        local.get $end
        i32.ge_u
        br_if $parse_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        local.tee $byte
        call $__sarif_is_ascii_digit
        i32.eqz
        br_if $parse_done
        local.get $result
        i64.const 10
        i64.mul
        local.get $byte
        i32.const 48
        i32.sub
        i64.extend_i32_u
        i64.add
        local.set $result
        i32.const 1
        local.set $has_digit
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $parse
      end
    end
    local.get $has_digit
    i32.eqz
    if
      unreachable
    end
    local.get $start
    local.get $end
    i32.ne
    if
      unreachable
    end
    local.get $negative
    if
      i64.const 0
      local.get $result
      i64.sub
      return
    end
    local.get $result
  )
  (func $__sarif_parse_i32_range (param $text i64) (param $start i64) (param $end i64) (result i64)
    local.get $text
    local.get $start
    local.get $end
    call $__sarif_text_slice
    call $__sarif_parse_i32
  )
  (func $__sarif_parse_f64 (param $text i64) (result f64)
    (local $ptr i32)
    (local $start i32)
    (local $end i32)
    (local $negative i32)
    (local $result f64)
    (local $scale f64)
    (local $byte i32)
    (local $has_digit i32)
    local.get $text
    i32.wrap_i64
    local.set $ptr
    i32.const 0
    local.set $start
    local.get $text
    call $__sarif_text_len_i32
    local.set $end
    block $trim_start_done
      loop $trim_start
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_start_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_start_done
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $trim_start
      end
    end
    block $trim_end_done
      loop $trim_end
        local.get $start
        local.get $end
        i32.ge_u
        br_if $trim_end_done
        local.get $ptr
        local.get $end
        i32.const 1
        i32.sub
        i32.add
        i32.load8_u
        call $__sarif_is_ascii_space
        i32.eqz
        br_if $trim_end_done
        local.get $end
        i32.const 1
        i32.sub
        local.set $end
        br $trim_end
      end
    end
    local.get $start
    local.get $end
    i32.ge_u
    if
      unreachable
    end
    i32.const 0
    local.set $negative
    local.get $ptr
    local.get $start
    i32.add
    i32.load8_u
    local.tee $byte
    i32.const 45
    i32.eq
    if
      i32.const 1
      local.set $negative
      local.get $start
      i32.const 1
      i32.add
      local.set $start
    else
      local.get $byte
      i32.const 43
      i32.eq
      if
        local.get $start
        i32.const 1
        i32.add
        local.set $start
      end
    end
    f64.const 0
    local.set $result
    i32.const 0
    local.set $has_digit
    block $whole_done
      loop $whole
        local.get $start
        local.get $end
        i32.ge_u
        br_if $whole_done
        local.get $ptr
        local.get $start
        i32.add
        i32.load8_u
        local.tee $byte
        call $__sarif_is_ascii_digit
        i32.eqz
        br_if $whole_done
        local.get $result
        f64.const 10
        f64.mul
        local.get $byte
        i32.const 48
        i32.sub
        f64.convert_i32_u
        f64.add
        local.set $result
        i32.const 1
        local.set $has_digit
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        br $whole
      end
    end
    local.get $start
    local.get $end
    i32.lt_u
    if
      local.get $ptr
      local.get $start
      i32.add
      i32.load8_u
      i32.const 46
      i32.eq
      if
        local.get $start
        i32.const 1
        i32.add
        local.set $start
        f64.const 10
        local.set $scale
        block $fraction_done
          loop $fraction
            local.get $start
            local.get $end
            i32.ge_u
            br_if $fraction_done
            local.get $ptr
            local.get $start
            i32.add
            i32.load8_u
            local.tee $byte
            call $__sarif_is_ascii_digit
            i32.eqz
            br_if $fraction_done
            local.get $result
            local.get $byte
            i32.const 48
            i32.sub
            f64.convert_i32_u
            local.get $scale
            f64.div
            f64.add
            local.set $result
            local.get $scale
            f64.const 10
            f64.mul
            local.set $scale
            i32.const 1
            local.set $has_digit
            local.get $start
            i32.const 1
            i32.add
            local.set $start
            br $fraction
          end
        end
      end
    end
    local.get $has_digit
    i32.eqz
    if
      unreachable
    end
    local.get $start
    local.get $end
    i32.ne
    if
      unreachable
    end
    local.get $negative
    if
      f64.const -1
      local.get $result
      f64.mul
      return
    end
    local.get $result
  )
  (func $__sarif_text_from_f64_fixed (param $value f64) (param $digits i64) (result i64)
    (local $precision i32) (local $negative i32) (local $scratch i32) (local $pos i32)
    (local $int_val i64) (local $mult f64) (local $i i32) (local $digit i32)
    (local $dest i32) (local $len i32) (local $frac_scaled i64) (local $frac_len i32)
    (local $frac_start i32)
    local.get $digits
    i32.wrap_i64
    local.tee $precision
    i32.const 0
    i32.lt_s
    if
      i32.const 0
      local.set $precision
    end
    local.get $precision
    i32.const 20
    i32.gt_s
    if
      i32.const 20
      local.set $precision
    end
    local.get $value
    local.get $value
    f64.ne
    if
      i32.const 3
      call $alloc
      local.tee $scratch
      i32.const 78
      i32.store8
      local.get $scratch
      i32.const 97
      i32.store8 offset=1
      local.get $scratch
      i32.const 78
      i32.store8 offset=2
      local.get $scratch
      i64.extend_i32_u
      i64.const 3
      i64.const 32
      i64.shl
      i64.or
      return
    end
    local.get $value
    f64.const 0
    f64.lt
    local.set $negative
    local.get $negative
    if
      local.get $value
      f64.neg
      local.set $value
    end
    local.get $value
    i64.trunc_f64_s
    local.set $int_val
    i32.const 64
    call $alloc
    local.set $scratch
    i32.const 63
    local.set $pos
    local.get $precision
    i32.const 0
    i32.gt_s
    if
      f64.const 1
      local.set $mult
      i32.const 0
      local.set $i
      block $scale_loop_end
        loop $scale_loop
          local.get $i
          local.get $precision
          i32.ge_s
          br_if $scale_loop_end
          local.get $mult
          f64.const 10
          f64.mul
          local.set $mult
          local.get $i
          i32.const 1
          i32.add
          local.set $i
          br $scale_loop
        end
      end
      local.get $value
      local.get $int_val
      f64.convert_i64_s
      f64.sub
      local.get $mult
      f64.mul
      f64.nearest
      i64.trunc_f64_s
      local.tee $frac_scaled
      local.get $mult
      i64.trunc_f64_s
      i64.ge_s
      if
        local.get $int_val
        i64.const 1
        i64.add
        local.set $int_val
        local.get $frac_scaled
        local.get $mult
        i64.trunc_f64_s
        i64.sub
        local.set $frac_scaled
      end
      local.get $frac_scaled
      i64.const 0
      i64.lt_s
      if
        i64.const 0
        local.set $frac_scaled
      end
      local.get $precision
      local.set $frac_len
      local.get $pos
      local.set $frac_start
      local.get $frac_scaled
      i64.const 0
      i64.eq
      if
        local.get $frac_start
        local.set $pos
      else
        block $frac_digits_end
          loop $frac_digits
            local.get $frac_scaled
            i64.const 0
            i64.eq
            br_if $frac_digits_end
            local.get $frac_scaled
            i64.const 10
            i64.rem_u
            i64.const 48
            i64.add
            i32.wrap_i64
            local.set $digit
            local.get $scratch
            local.get $pos
            i32.add
            local.get $digit
            i32.store8
            local.get $frac_scaled
            i64.const 10
            i64.div_u
            local.set $frac_scaled
            local.get $pos
            i32.const 1
            i32.sub
            local.set $pos
            br $frac_digits
          end
        end
      end
      block $pad_end
        loop $pad_loop
          local.get $pos
          local.get $frac_start
          local.get $frac_len
          i32.sub
          i32.ge_s
          br_if $pad_end
          local.get $scratch
          local.get $pos
          i32.add
          i32.const 48
          i32.store8
          local.get $pos
          i32.const 1
          i32.sub
          local.set $pos
          br $pad_loop
        end
      end
      local.get $scratch
      local.get $pos
      i32.add
      i32.const 46
      i32.store8
      local.get $pos
      i32.const 1
      i32.sub
      local.set $pos
    end
    local.get $int_val
    i64.const 0
    i64.eq
    if
      local.get $scratch
      local.get $pos
      i32.add
      i32.const 48
      i32.store8
      local.get $pos
      i32.const 1
      i32.sub
      local.set $pos
    else
      block $int_digits_end
        loop $int_digits
          local.get $int_val
          i64.const 0
          i64.eq
          br_if $int_digits_end
          local.get $int_val
          i64.const 10
          i64.rem_u
          i64.const 48
          i64.add
          i32.wrap_i64
          local.set $digit
          local.get $scratch
          local.get $pos
          i32.add
          local.get $digit
          i32.store8
          local.get $int_val
          i64.const 10
          i64.div_u
          local.set $int_val
          local.get $pos
          i32.const 1
          i32.sub
          local.set $pos
          br $int_digits
        end
      end
    end
    local.get $negative
    if
      local.get $scratch
      local.get $pos
      i32.add
      i32.const 45
      i32.store8
      local.get $pos
      i32.const 1
      i32.sub
      local.set $pos
    end
    local.get $pos
    i32.const 1
    i32.add
    local.set $pos
    i32.const 63
    local.get $pos
    i32.sub
    i32.const 1
    i32.add
    local.tee $len
    call $alloc
    local.set $dest
    i32.const 0
    local.set $i
    block $copy_end
      loop $copy
        local.get $i
        local.get $len
        i32.ge_s
        br_if $copy_end
        local.get $dest
        local.get $i
        i32.add
        local.get $scratch
        local.get $pos
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
    local.get $dest
    i64.extend_i32_u
    local.get $len
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.or
  )
