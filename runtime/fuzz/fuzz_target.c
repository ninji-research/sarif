#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdlib.h>

// Rename the runtime's main to avoid conflict with libFuzzer's main.
// Provide a stub sarif_user_main so the renamed main can link if invoked.
void sarif_user_main(void) {}
#define main __runtime_main_disabled

// Include the full runtime source so all static functions are visible.
#include "../sarif_runtime.c"

// Undefine main so our code below doesn't get the renamed symbol.
#undef main

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

static int64_t extract_i64(const uint8_t* data, size_t len, size_t offset,
                            int64_t default_val) {
    if (offset + 8 > len) return default_val;
    int64_t val;
    memcpy(&val, data + offset, 8);
    if (val < 0) val = -val;
    return val;
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
    if (list_ptr) {
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
// Fuzz target entry point.
// ---------------------------------------------------------------------------

int LLVMFuzzerTestOneInput(const uint8_t* data, size_t size) {
    if (size < 1) return 0;

    // Reset arena for each fuzz iteration.
    sarif_record_chunks = NULL;
    sarif_record_current = NULL;
    sarif_alloc_scope_stack = NULL;

    // Open an alloc scope so arena allocations are cleaned up on exit.
    sarif_alloc_push();

    uint8_t op = data[0];
    const uint8_t* payload = data + 1;
    size_t payload_len = size - 1;

    switch (op & 0x1f) {
        case 0: {
            size_t half = payload_len / 2;
            unsigned char* left = make_text(payload, half, 4096);
            unsigned char* right = make_text(payload + half, payload_len - half, 4096);
            if (left && right) {
                void* result = sarif_text_concat(left, right);
                (void)result;
            }
            break;
        }
        case 1: {
            unsigned char* text = make_text(payload, payload_len, 4096);
            uint64_t start = extract_u64(payload, payload_len, 0, 0);
            uint64_t end = extract_u64(payload, payload_len, 8, payload_len);
            if (text) {
                void* slice = sarif_text_slice(text, start, end);
                (void)slice;
            }
            break;
        }
        case 2: {
            unsigned char* bytes = make_text(payload, payload_len, 4096);
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
                unsigned char* text = make_text(payload, payload_len, 256);
                if (text) {
                    void* tmp = sarif_text_builder_append(builder, text);
                    if (tmp) builder = tmp;
                }
                void* tmp = sarif_text_builder_append_codepoint(
                    builder, extract_i64(payload, payload_len, 0, 65));
                if (tmp) builder = tmp;
                tmp = sarif_text_builder_append_ascii(
                    builder, extract_i64(payload, payload_len, 0, 65) & 0x7f);
                if (tmp) builder = tmp;
                tmp = sarif_text_builder_append_i32(
                    builder, extract_i64(payload, payload_len, 0, 42));
                if (tmp) builder = tmp;
                void* result = sarif_text_builder_finish(builder);
                (void)result;
            }
            break;
        }
        case 4: {
            void* builder = sarif_text_builder_new();
            if (builder) {
                unsigned char* text = make_text(payload, payload_len, 256);
                if (text) {
                    int64_t start = (int64_t)extract_u64(payload, payload_len, 0, 0);
                    int64_t end = (int64_t)extract_u64(payload, payload_len, 8, payload_len);
                    void* tmp = sarif_text_builder_append_slice(builder, text, start, end);
                    if (tmp) builder = tmp;
                }
                void* result = sarif_text_builder_finish(builder);
                (void)result;
            }
            break;
        }
        case 5: {
            int64_t initial_len = (int64_t)(extract_u64(payload, payload_len, 0, 0) & 0xff);
            void* list = sarif_list_new(initial_len, extract_u64(payload, payload_len, 8, 0));
            if (list) {
                int64_t push_len = (int64_t)(extract_u64(payload, payload_len, 16, 0) & 0xff);
                sarif_list_push(list, push_len, extract_u64(payload, payload_len, 24, 0));
                sarif_list_sort_text(list, push_len);
            }
            cleanup_list(list);
            break;
        }
        case 6: {
            unsigned char* text = make_text(payload, payload_len, 128);
            if (text) {
                sarif_parse_i32(text);
                sarif_parse_i32_range(text, extract_i64(payload, payload_len, 0, 0),
                                      extract_i64(payload, payload_len, 8, payload_len));
                sarif_parse_f64(text);
            }
            break;
        }
        case 7: {
            size_t half = payload_len / 2;
            unsigned char* left = make_text(payload, half, 256);
            unsigned char* right = make_text(payload + half, payload_len - half, 256);
            if (left && right) {
                sarif_text_cmp(left, right);
                sarif_text_eq(left, right);
                sarif_text_eq_range(left, extract_i64(payload, payload_len, 0, 0),
                                    extract_i64(payload, payload_len, 8, payload_len), right);
            }
            break;
        }
        case 8: {
            unsigned char* text = make_text(payload, payload_len, 1024);
            if (text) {
                int64_t start = extract_i64(payload, payload_len, 0, 0);
                int64_t end = extract_i64(payload, payload_len, 8, payload_len);
                sarif_text_find_byte_range(text, start, end,
                                           extract_i64(payload, payload_len, 16, 10));
                sarif_text_line_end(text, start);
                sarif_text_next_line(text, start);
                sarif_text_field_end(text, start, end,
                                     extract_i64(payload, payload_len, 16, 44));
                sarif_text_next_field(text, start, end,
                                      extract_i64(payload, payload_len, 16, 44));
            }
            break;
        }
        case 9: {
            double value = extract_f64(payload, payload_len, 0);
            int64_t digits = extract_i64(payload, payload_len, 8, 6);
            void* result = sarif_text_from_f64_fixed(value, digits);
            (void)result;
            break;
        }
        case 10: {
            void* index = sarif_text_index_new();
            if (index) {
                unsigned char* key = make_text(payload, payload_len, 64);
                if (key) {
                    uint64_t key_handle = (uint64_t)(uintptr_t)key;
                    sarif_text_index_set(index, key_handle,
                                         extract_i64(payload, payload_len, 0, 0));
                    sarif_text_index_get(index, key_handle);
                    sarif_text_index_get_or_insert(index, key_handle,
                                                   extract_i64(payload, payload_len, 8, 0));
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
                unsigned char* big_text = make_text(payload, payload_len, 8192);
                if (big_text) {
                    int64_t start = (int64_t)extract_u64(payload, payload_len, 0, 0);
                    int64_t end = (int64_t)extract_u64(payload, payload_len, 8, payload_len);
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

    // Clean up arena allocations from this iteration.
    // Heap-allocated objects (builders, lists, indices) may still leak,
    // but the arena is the primary allocator for most paths.
    sarif_alloc_pop();

    return 0;
}
