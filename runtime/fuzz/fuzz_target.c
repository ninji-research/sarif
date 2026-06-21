#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdlib.h>
#include <limits.h>

#if UINTPTR_MAX > UINT64_MAX
#error "fuzz_target.c requires pointer width <= 64 bits because pointer handles are stored in uint64_t slots."
#endif

// Rename the runtime's main to avoid conflict with libFuzzer's main.
// Provide a stub sarif_user_main so the renamed main can link if invoked.
void sarif_user_main(void) {}
#define main __runtime_main_disabled

// Include the full runtime source so all static functions are visible.
#include "../sarif_runtime.c"

// Undefine main so our code below doesn't get the renamed symbol.
#undef main

// Stub declarations of the effect table needed by the runtime.
// Keep one inert NULL entry for portable non-empty storage in this fuzz harness.
const struct SarifEffectHandler sarif_effect_table[1] = { {NULL, NULL, NULL} };
const size_t sarif_effect_table_len = 1;

// Default fallback value used when appending i32 fields.
#define DEFAULT_APPEND_I32_VALUE 42

// Default delimiter used for field-oriented text helpers (ASCII ',').
#define DEFAULT_FIELD_DELIMITER 44

// Default fallback character value used in fuzz harness (ASCII 'A').
#define DEFAULT_APPEND_CHAR_VALUE 65

// Default line-ending byte used for text search checks (ASCII '\n').
#define DEFAULT_LINE_END_BYTE 10

// Default floating-point precision format limit.
#define DEFAULT_F64_PRECISION 6

// Named text-size limits used for fuzz-generated length-prefixed text values.
#define MAX_TEXT_SMALL 64U
#define MAX_TEXT_MEDIUM 128U
#define MAX_TEXT_DEFAULT 256U
#define MAX_TEXT_LARGE 1024U
#define MAX_TEXT_XLARGE 4096U
#define MAX_TEXT_XXLARGE 8192U

// Fixed number of entries used when partitioning payloads for list fuzzing.
#define FUZZ_LIST_SIZE 8U

// Mask to ensure the character payload is constrained to valid 7-bit ASCII.
#define ASCII_7BIT_MASK 0x7f

// This harness stores pointer payloads in uint64_t list slots.
// Enforce that uintptr_t fits in uint64_t to avoid truncation on exotic targets.
_Static_assert(sizeof(uintptr_t) <= sizeof(uint64_t),
               "uintptr_t must fit in uint64_t for SarifList value storage");

// ---------------------------------------------------------------------------
// Helper functions for constructing test inputs from fuzz data.
// ---------------------------------------------------------------------------

// Create a length-prefixed text from raw bytes, clamped to max_len.
static unsigned char* make_text(const uint8_t* data, size_t len, size_t max_len) {
    size_t text_len = len < max_len ? len : max_len;
    unsigned char* text = sarif_text_alloc((uint64_t)text_len);
    if (text == NULL) return NULL;
    if (text_len > 0) {
        memcpy(text + 8, data, text_len);
    }
    return text;
}

// Convert an opaque pointer to the runtime's u64 handle representation.
static uint64_t ptr_to_u64_handle(const void* ptr) {
    return (uint64_t)(uintptr_t)ptr;
}

// Safely cast size_t to int64_t, clamping to INT64_MAX to prevent overflow.
static inline int64_t safe_size_to_i64(size_t val) {
    const size_t int64_max_as_size = (size_t)INT64_MAX;
    return val > int64_max_as_size ? INT64_MAX : (int64_t)val;
}

// Return the non-negative magnitude of a signed 64-bit value as int64_t.
// For INT64_MIN, returns INT64_MAX (saturating, since |INT64_MIN| is not representable).
static int64_t extract_i64_magnitude_saturating(const uint8_t* data, size_t len, size_t offset,
                                                int64_t default_val) {
    if (offset + 8 > len) return default_val;
    int64_t val;
    memcpy(&val, data + offset, 8);
    if (val < 0) {
        if (val == INT64_MIN) {
            val = INT64_MAX; // Intentional saturating clamp for unrepresentable |INT64_MIN|
        } else {
            val = -val;      // Safe negation for all other negative int64_t values
        }
    }
    return val;
}

// Extract a fuzz-derived ASCII byte constrained to 7-bit range [0, 127].
static uint8_t extract_ascii_7bit(const uint8_t* data, size_t len, size_t off, int64_t defv) {
    int64_t raw = extract_i64_magnitude_saturating(data, len, off, defv);
    return (uint8_t)(raw & ASCII_7BIT_MASK);
}

// Saturating conversion helper to keep int32 conversion logic consistent.
static int32_t safe_i64_to_i32(int64_t value) {
    if (value > INT32_MAX) return INT32_MAX;
    if (value < INT32_MIN) return INT32_MIN;
    return (int32_t)value;
}

static uint64_t extract_u64(const uint8_t* data, size_t len, size_t offset,
                             uint64_t default_val) {
    if (offset + 8 > len) return default_val;
    uint64_t val;
    memcpy(&val, data + offset, 8);
    return val;
}

static double extract_f64(const uint8_t* data, size_t len, size_t offset) {
    if (offset + 8 > len) return 0.0;
    double val;
    memcpy(&val, data + offset, 8);
    return val;
}

// ---------------------------------------------------------------------------
// Cleanup helpers for heap-allocated objects (not arena-managed).
// ---------------------------------------------------------------------------

static void cleanup_list(void* list_ptr) {
    if (list_ptr && list_ptr != &sarif_empty_list) {
        SarifList* list = (SarifList*)list_ptr;
        free(list->values);
        free(list);
    }
}

static void cleanup_text_index(void* index_ptr) {
    if (index_ptr) {
        SarifTextIndex* index = (SarifTextIndex*)index_ptr;
        free(index->entries);
        free(index);
    }
}

// ---------------------------------------------------------------------------
// Fuzz target isolation helpers.
// ---------------------------------------------------------------------------

static void fuzz_runtime_init_iteration(void) {
    // Reset arena for each fuzz iteration.
    sarif_record_chunks = NULL;
    sarif_record_current = NULL;
    sarif_scope_depth = 0;
    sarif_scope_overflow = NULL;

    // Open an alloc scope so arena allocations are cleaned up on exit.
    sarif_alloc_push();
}

static void fuzz_runtime_end_iteration(void) {
    // Clean up arena allocations from this iteration.
    sarif_alloc_pop();

    // Free stdin cache if it was dynamically allocated
    if (sarif_stdin_cache != NULL) {
        free(sarif_stdin_cache);
        sarif_stdin_cache = NULL;
    }
}

// ---------------------------------------------------------------------------
// Fuzz target entry point.
// ---------------------------------------------------------------------------

// Mask to extract the opcode from the first byte of fuzz input.
// We use a 5-bit mask (0x1fU) providing 32 opcode slots (0-31) to accommodate
// future API extensions. Currently, only opcodes 0-12 are implemented to cover
// the existing core runtime built-in functions. Opcodes 13-31 are safely ignored
// by the switch default case.
#define FUZZ_OP_MASK 0x1fU

#define FUZZ_MAX_IMPLEMENTED_OPCODE 12U
_Static_assert(FUZZ_MAX_IMPLEMENTED_OPCODE <= FUZZ_OP_MASK,
               "FUZZ_MAX_IMPLEMENTED_OPCODE must not exceed FUZZ_OP_MASK");

int LLVMFuzzerTestOneInput(const uint8_t* data, size_t size) {
    if (size < 1) return 0;

    fuzz_runtime_init_iteration();

    uint8_t op = data[0];
    const uint8_t* payload = data + 1;
    size_t payload_len = size - 1;

    switch (op & FUZZ_OP_MASK) {
        case 0: {
            size_t half = payload_len / 2;
            unsigned char* left = make_text(payload, half, MAX_TEXT_XLARGE);
            unsigned char* right = make_text(payload + half, payload_len - half, MAX_TEXT_XLARGE);
            if (left && right) {
                void* result = sarif_text_concat(left, right);
                (void)result;
            }
            break;
        }
        case 1: {
            unsigned char* text = make_text(payload, payload_len, MAX_TEXT_XLARGE);
            uint64_t start = extract_u64(payload, payload_len, 0, 0);
            uint64_t end = extract_u64(payload, payload_len, 8, payload_len);
            if (text) {
                void* slice = sarif_text_slice(text, start, end);
                (void)slice;
            }
            break;
        }
        case 2: {
            unsigned char* bytes = make_text(payload, payload_len, MAX_TEXT_XLARGE);
            uint64_t start = extract_u64(payload, payload_len, 0, 0);
            uint64_t end = extract_u64(payload, payload_len, 8, payload_len);
            if (bytes) {
                void* slice = sarif_bytes_slice(bytes, start, end);
                (void)slice;
            }
            break;
        }
        case 3: {
            void* builder = sarif_text_builder_new();
            if (builder) {
                unsigned char* text = make_text(payload, payload_len, MAX_TEXT_DEFAULT);
                if (text) {
                    void* tmp = sarif_text_builder_append(builder, text);
                    if (tmp) builder = tmp;
                }
                void* tmp = sarif_text_builder_append_codepoint(
                    builder, extract_i64_magnitude_saturating(payload, payload_len, 0, DEFAULT_APPEND_CHAR_VALUE));
                if (tmp) builder = tmp;
                uint8_t ascii_char = extract_ascii_7bit(payload, payload_len, 8, DEFAULT_APPEND_CHAR_VALUE);
                tmp = sarif_text_builder_append_ascii(builder, ascii_char);
                if (tmp) builder = tmp;
                int64_t append_i32_raw = extract_i64_magnitude_saturating(payload, payload_len, 16, DEFAULT_APPEND_I32_VALUE);
                tmp = sarif_text_builder_append_i32(builder, safe_i64_to_i32(append_i32_raw));
                if (tmp) builder = tmp;
                void* result = sarif_text_builder_finish(builder);
                (void)result;
            }
            break;
        }
        case 4: {
            void* builder = sarif_text_builder_new();
            if (builder) {
                unsigned char* text = make_text(payload, payload_len, MAX_TEXT_DEFAULT);
                if (text) {
                    int64_t start = extract_i64_magnitude_saturating(payload, payload_len, 0, 0);
                    int64_t end = extract_i64_magnitude_saturating(payload, payload_len, 8, safe_size_to_i64(payload_len));
                    void* tmp = sarif_text_builder_append_slice(builder, text, start, end);
                    if (tmp) builder = tmp;
                }
                void* result = sarif_text_builder_finish(builder);
                (void)result;
            }
            break;
        }
        case 5: {
            // Create a list with valid text handles from the fuzzer payload.
            size_t base = payload_len / FUZZ_LIST_SIZE;
            size_t remainder = payload_len % FUZZ_LIST_SIZE;
            size_t offset = 0;
            unsigned char* texts[FUZZ_LIST_SIZE] = {NULL};
            for (size_t i = 0; i < FUZZ_LIST_SIZE; i++) {
                size_t seg_len = base + (i < remainder ? 1 : 0);
                if (seg_len > 0) {
                    texts[i] = make_text(payload + offset, seg_len, MAX_TEXT_SMALL);
                    offset += seg_len;
                } else {
                    texts[i] = sarif_text_alloc(0);
                }
            }
            void* list = sarif_list_new(FUZZ_LIST_SIZE, 0);
            if (list) {
                SarifList* l = (SarifList*)list;
                for (size_t i = 0; i < FUZZ_LIST_SIZE; i++) {
                    l->values[i] = ptr_to_u64_handle(texts[i]);
                }
                unsigned char* extra_text = make_text(payload, payload_len, MAX_TEXT_SMALL);
                if (extra_text) {
                    // sarif_list_push may return a replacement list pointer; NULL indicates push failure.
                    void* pushed_list = sarif_list_push(list, FUZZ_LIST_SIZE, ptr_to_u64_handle(extra_text));
                    if (pushed_list) {
                        list = pushed_list;
                        sarif_list_sort_text(list, FUZZ_LIST_SIZE + 1);
                    } else {
                        // Keep and sort the original list when push fails.
                        sarif_list_sort_text(list, FUZZ_LIST_SIZE);
                    }
                } else {
                    // Skip push when text allocation fails; sort the original valid entries.
                    sarif_list_sort_text(list, FUZZ_LIST_SIZE);
                }
            }
            cleanup_list(list);
            break;
        }
        case 6: {
            unsigned char* text = make_text(payload, payload_len, MAX_TEXT_MEDIUM);
            if (text) {
                sarif_parse_i32(text);
                int64_t start = extract_i64_magnitude_saturating(payload, payload_len, 0, 0);
                int64_t end = extract_i64_magnitude_saturating(payload, payload_len, 8, safe_size_to_i64(payload_len));
                sarif_parse_i32_range(text, start, end);
                sarif_parse_f64(text);
            }
            break;
        }
        case 7: {
            size_t half = payload_len / 2;
            unsigned char* left = make_text(payload, half, MAX_TEXT_DEFAULT);
            unsigned char* right = make_text(payload + half, payload_len - half, MAX_TEXT_DEFAULT);
            if (left && right) {
                sarif_text_cmp(left, right);
                sarif_text_eq(left, right);
                int64_t range_start = extract_i64_magnitude_saturating(payload, payload_len, 0, 0);
                int64_t range_end = extract_i64_magnitude_saturating(payload, payload_len, 8, safe_size_to_i64(payload_len));
                sarif_text_eq_range(left, range_start, range_end, right);
            }
            break;
        }
        case 8: {
            unsigned char* text = make_text(payload, payload_len, MAX_TEXT_LARGE);
            if (text) {
                int64_t start = extract_i64_magnitude_saturating(payload, payload_len, 0, 0);
                int64_t end = extract_i64_magnitude_saturating(payload, payload_len, 8, safe_size_to_i64(payload_len));
                uint8_t needle = (uint8_t)(extract_i64_magnitude_saturating(payload, payload_len, 16, DEFAULT_LINE_END_BYTE) & 0xFF); // Intentional low-byte truncation for byte-oriented API.
                uint8_t field_delim = (uint8_t)(extract_i64_magnitude_saturating(payload, payload_len, 24, DEFAULT_FIELD_DELIMITER) & 0xFF); // Intentional low-byte truncation for byte-oriented API.
                sarif_text_find_byte_range(text, start, end, needle);
                sarif_text_line_end(text, start);
                sarif_text_next_line(text, start);
                sarif_text_field_end(text, start, end, field_delim);
                sarif_text_next_field(text, start, end, field_delim);
            }
            break;
        }
        case 9: {
            double value = extract_f64(payload, payload_len, 0);
            int64_t digits = extract_i64_magnitude_saturating(payload, payload_len, 8, DEFAULT_F64_PRECISION);
            void* result = sarif_text_from_f64_fixed(value, digits);
            (void)result;
            break;
        }
        case 10: {
            void* index = sarif_text_index_new();
            if (index) {
                unsigned char* key = make_text(payload, payload_len, MAX_TEXT_SMALL);
                if (key) {
                    uint64_t key_handle = ptr_to_u64_handle(key);
                    sarif_text_index_set(index, key_handle,
                                         extract_i64_magnitude_saturating(payload, payload_len, 0, 0));
                    sarif_text_index_get(index, key_handle);
                    sarif_text_index_get_or_insert(index, key_handle,
                                                   extract_i64_magnitude_saturating(payload, payload_len, 8, 0));
                }
            }
            cleanup_text_index(index);
            break;
        }
        case 11: {
            unsigned char* empty = sarif_text_alloc(0);
            if (empty) {
                sarif_text_cmp(empty, empty);
                sarif_text_concat(empty, empty);
                sarif_text_slice(empty, 0, 0);
                sarif_text_find_byte_range(empty, 0, 0, 0);
                sarif_text_line_end(empty, 0);
                sarif_text_next_line(empty, 0);
            }
            break;
        }
        case 12: {
            void* builder = sarif_text_builder_new();
            if (builder) {
                unsigned char* short_text = sarif_text_alloc(5);
                if (short_text) {
                    memcpy(short_text + 8, "hello", 5);
                    void* tmp = sarif_text_builder_append(builder, short_text);
                    if (tmp) builder = tmp;
                }
                unsigned char* big_text = make_text(payload, payload_len, MAX_TEXT_XXLARGE);
                if (big_text) {
                    int64_t start = extract_i64_magnitude_saturating(payload, payload_len, 0, 0);
                    int64_t end = extract_i64_magnitude_saturating(payload, payload_len, 8, safe_size_to_i64(payload_len));
                    void* tmp = sarif_text_builder_append_slice(builder, big_text, start, end);
                    if (tmp) builder = tmp;
                }
                void* result = sarif_text_builder_finish(builder);
                (void)result;
            }
            break;
        }
        default:
            break;
    }

    fuzz_runtime_end_iteration();

    return 0;
}
