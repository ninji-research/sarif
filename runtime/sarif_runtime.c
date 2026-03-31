#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <errno.h>
#include <limits.h>
#include <string.h>

#ifndef SARIF_MAIN_KIND
#define SARIF_MAIN_KIND 0
#endif

#ifndef SARIF_MAIN_PRINT
#define SARIF_MAIN_PRINT 0
#endif

static int sarif_argc = 0;
static char** sarif_argv = NULL;
static unsigned char* sarif_stdin_cache = NULL;
static unsigned char sarif_empty_text[8] = {0};
static int sarif_write_text_blob(const unsigned char* text, int newline);

#define SARIF_RECORD_ALIGN 16u
#define SARIF_RECORD_ARENA_CHUNK_SIZE (1u << 20)
#define SARIF_TEXT_BUILDER_INITIAL_CAP 4096u
#define SARIF_STDIN_CHUNK_SIZE 16384u

typedef struct SarifRecordDesc SarifRecordDesc;
typedef struct SarifEnumDesc SarifEnumDesc;
typedef struct SarifVariantDesc SarifVariantDesc;
typedef struct SarifTextBuilder SarifTextBuilder;
typedef struct SarifList SarifList;
typedef struct SarifRecordChunk SarifRecordChunk;
typedef struct SarifAllocScope SarifAllocScope;

typedef struct SarifFieldDesc {
    const char* name;
    uint32_t kind;
    uint64_t offset;
    const SarifRecordDesc* record;
    const SarifEnumDesc* enum_desc;
} SarifFieldDesc;

struct SarifRecordDesc {
    const char* name;
    uint64_t field_count;
    const SarifFieldDesc* fields;
};

struct SarifVariantDesc {
    const char* name;
    uint32_t payload_kind;
    const SarifRecordDesc* record;
    const SarifEnumDesc* enum_desc;
};

struct SarifEnumDesc {
    const char* name;
    uint64_t variant_count;
    const SarifVariantDesc* variants;
};

struct SarifTextBuilder {
    uint64_t len;
    uint64_t cap;
    unsigned char* bytes;
};

struct SarifRecordChunk {
    SarifRecordChunk* next;
    size_t used;
    size_t cap;
    unsigned char data[];
};

struct SarifAllocScope {
    SarifRecordChunk* chunk;
    size_t used;
    SarifAllocScope* prev;
};

// SarifList uses SoA (Struct of Arrays) layout for cache-oblivious access
// This allows SIMD-friendly patterns and better memory locality
struct SarifList {
    uint64_t len;
    uint64_t* values;  // elements stored as bitcast handles
};

extern const SarifRecordDesc* sarif_get_main_record_desc(void);
extern const SarifEnumDesc* sarif_get_main_enum_desc(void);

#if SARIF_MAIN_KIND == 1
extern int32_t sarif_user_main(void);
#elif SARIF_MAIN_KIND == 2
extern uint32_t sarif_user_main(void);
#elif SARIF_MAIN_KIND == 3
extern uintptr_t sarif_user_main(void);
#elif SARIF_MAIN_KIND == 4
extern uintptr_t sarif_user_main(void);
#elif SARIF_MAIN_KIND == 5
extern uint64_t sarif_user_main(void);
#elif SARIF_MAIN_KIND == 6
extern double sarif_user_main(void);
#else
extern void sarif_user_main(void);
#endif

static SarifRecordChunk* sarif_record_chunks = NULL;
static SarifRecordChunk* sarif_record_current = NULL;
static SarifAllocScope* sarif_alloc_scope_stack = NULL;

void* sarif_record_alloc(uint64_t size) {
    SarifRecordChunk* chunk = NULL;
    size_t aligned = 0;
    size_t min_cap = 0;
    if (size == 0 || size > (uint64_t)SIZE_MAX) {
        return NULL;
    }
    aligned = (size_t)size;
    if (aligned > SIZE_MAX - (SARIF_RECORD_ALIGN - 1u)) {
        return NULL;
    }
    aligned = (aligned + (SARIF_RECORD_ALIGN - 1u)) & ~(SARIF_RECORD_ALIGN - 1u);
    chunk = sarif_record_current;
    if (chunk != NULL && aligned <= chunk->cap - chunk->used) {
        void* ptr = chunk->data + chunk->used;
        chunk->used += aligned;
        return ptr;
    }
    min_cap = aligned > SARIF_RECORD_ARENA_CHUNK_SIZE ? aligned : SARIF_RECORD_ARENA_CHUNK_SIZE;
    if (min_cap > SIZE_MAX - sizeof(SarifRecordChunk)) {
        return NULL;
    }
    chunk = malloc(sizeof(SarifRecordChunk) + min_cap);
    if (chunk == NULL) {
        return NULL;
    }
    chunk->next = NULL;
    chunk->used = aligned;
    chunk->cap = min_cap;
    if (sarif_record_current != NULL) {
        sarif_record_current->next = chunk;
    } else {
        sarif_record_chunks = chunk;
    }
    sarif_record_current = chunk;
    return chunk->data;
}

void sarif_alloc_push(void) {
    SarifAllocScope* scope = malloc(sizeof(SarifAllocScope));
    if (scope == NULL) {
        return;
    }
    scope->chunk = sarif_record_current;
    scope->used = sarif_record_current == NULL ? 0u : sarif_record_current->used;
    scope->prev = sarif_alloc_scope_stack;
    sarif_alloc_scope_stack = scope;
}

void sarif_alloc_pop(void) {
    SarifAllocScope* scope = sarif_alloc_scope_stack;
    SarifRecordChunk* chunk = NULL;
    SarifRecordChunk* next = NULL;
    if (scope == NULL) {
        return;
    }
    sarif_alloc_scope_stack = scope->prev;
    if (scope->chunk == NULL) {
        chunk = sarif_record_chunks;
        while (chunk != NULL) {
            next = chunk->next;
            free(chunk);
            chunk = next;
        }
        sarif_record_chunks = NULL;
        sarif_record_current = NULL;
        free(scope);
        return;
    }
    chunk = scope->chunk->next;
    scope->chunk->next = NULL;
    while (chunk != NULL) {
        next = chunk->next;
        free(chunk);
        chunk = next;
    }
    sarif_record_current = scope->chunk;
    sarif_record_current->used = scope->used;
    free(scope);
}

static void sarif_store_u64(unsigned char* base, uint64_t offset, uint64_t value) {
    unsigned char* bytes = base + offset;
    bytes[0] = (unsigned char)(value & 0xffu);
    bytes[1] = (unsigned char)((value >> 8) & 0xffu);
    bytes[2] = (unsigned char)((value >> 16) & 0xffu);
    bytes[3] = (unsigned char)((value >> 24) & 0xffu);
    bytes[4] = (unsigned char)((value >> 32) & 0xffu);
    bytes[5] = (unsigned char)((value >> 40) & 0xffu);
    bytes[6] = (unsigned char)((value >> 48) & 0xffu);
    bytes[7] = (unsigned char)((value >> 56) & 0xffu);
}

static uint64_t sarif_load_u64(const unsigned char* base, uint64_t offset) {
    const unsigned char* bytes = base + offset;
    return ((uint64_t)bytes[0]) |
           ((uint64_t)bytes[1] << 8) |
           ((uint64_t)bytes[2] << 16) |
           ((uint64_t)bytes[3] << 24) |
           ((uint64_t)bytes[4] << 32) |
           ((uint64_t)bytes[5] << 40) |
           ((uint64_t)bytes[6] << 48) |
           ((uint64_t)bytes[7] << 56);
}

static int sarif_is_utf8_continuation(unsigned char byte) {
    return (byte & 0xc0u) == 0x80u;
}

void* sarif_text_builder_new(void) {
    SarifTextBuilder* builder = calloc(1u, sizeof(SarifTextBuilder));
    return builder;
}

static SarifTextBuilder* sarif_text_builder_reserve(
    SarifTextBuilder* builder,
    uint64_t appended_len
) {
    uint64_t required = 0;
    uint64_t next_cap = 0;
    unsigned char* grown = NULL;
    if (builder == NULL) {
        return NULL;
    }
    if (appended_len == 0) {
        return builder;
    }
    if (builder->len > UINT64_MAX - appended_len) {
        return NULL;
    }
    required = builder->len + appended_len;
    if (required <= builder->cap) {
        return builder;
    }
    next_cap = builder->cap == 0 ? SARIF_TEXT_BUILDER_INITIAL_CAP : builder->cap;
    while (next_cap < required) {
        if (next_cap > UINT64_MAX / 2u) {
            next_cap = required;
            break;
        }
        next_cap *= 2u;
    }
    if (next_cap < required || next_cap > (uint64_t)SIZE_MAX) {
        return NULL;
    }
    grown = realloc(builder->bytes, (size_t)next_cap);
    if (grown == NULL) {
        return NULL;
    }
    builder->bytes = grown;
    builder->cap = next_cap;
    return builder;
}

void* sarif_text_builder_append(void* raw_builder, const unsigned char* text) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    uint64_t text_len = 0;
    if (builder == NULL || text == NULL) {
        return NULL;
    }
    text_len = sarif_load_u64(text, 0);
    if (text_len == 0) {
        return builder;
    }
    builder = sarif_text_builder_reserve(builder, text_len);
    if (builder == NULL) {
        return NULL;
    }
    memcpy(builder->bytes + builder->len, text + 8, (size_t)text_len);
    builder->len += text_len;
    return builder;
}

void* sarif_text_builder_append_codepoint(void* raw_builder, int64_t codepoint) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    unsigned char encoded[4];
    uint64_t encoded_len = 0;
    if (builder == NULL || codepoint < 0 || codepoint > 0x10ffff) {
        return NULL;
    }
    if (codepoint <= 0x7f) {
        encoded[0] = (unsigned char)codepoint;
        encoded_len = 1;
    } else if (codepoint <= 0x7ff) {
        encoded[0] = (unsigned char)(0xc0u | ((uint64_t)codepoint >> 6));
        encoded[1] = (unsigned char)(0x80u | ((uint64_t)codepoint & 0x3fu));
        encoded_len = 2;
    } else if (codepoint >= 0xd800 && codepoint <= 0xdfff) {
        return NULL;
    } else if (codepoint <= 0xffff) {
        encoded[0] = (unsigned char)(0xe0u | ((uint64_t)codepoint >> 12));
        encoded[1] = (unsigned char)(0x80u | (((uint64_t)codepoint >> 6) & 0x3fu));
        encoded[2] = (unsigned char)(0x80u | ((uint64_t)codepoint & 0x3fu));
        encoded_len = 3;
    } else {
        encoded[0] = (unsigned char)(0xf0u | ((uint64_t)codepoint >> 18));
        encoded[1] = (unsigned char)(0x80u | (((uint64_t)codepoint >> 12) & 0x3fu));
        encoded[2] = (unsigned char)(0x80u | (((uint64_t)codepoint >> 6) & 0x3fu));
        encoded[3] = (unsigned char)(0x80u | ((uint64_t)codepoint & 0x3fu));
        encoded_len = 4;
    }
    builder = sarif_text_builder_reserve(builder, encoded_len);
    if (builder == NULL) {
        return NULL;
    }
    memcpy(builder->bytes + builder->len, encoded, (size_t)encoded_len);
    builder->len += encoded_len;
    return builder;
}

void* sarif_text_builder_finish(void* raw_builder) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    unsigned char* text = NULL;
    if (builder == NULL) {
        return NULL;
    }
    if (builder->len > (uint64_t)SIZE_MAX - 8u) {
        free(builder->bytes);
        free(builder);
        return NULL;
    }
    text = malloc(8u + (size_t)builder->len);
    if (text == NULL) {
        free(builder->bytes);
        free(builder);
        return NULL;
    }
    sarif_store_u64(text, 0, builder->len);
    if (builder->len != 0) {
        memcpy(text + 8, builder->bytes, (size_t)builder->len);
    }
    free(builder->bytes);
    free(builder);
    return text;
}

void* sarif_list_new(int64_t len, uint64_t fill) {
    SarifList* vec = NULL;
    uint64_t index = 0;
    if (len < 0) {
        return NULL;
    }
    if ((uint64_t)len > (uint64_t)SIZE_MAX / sizeof(uint64_t)) {
        return NULL;
    }
    vec = calloc(1u, sizeof(SarifList));
    if (vec == NULL) {
        return NULL;
    }
    vec->len = (uint64_t)len;
    if (fill == 0) {
        vec->values = calloc((size_t)len, sizeof(uint64_t));
        if (vec->values == NULL) {
            free(vec);
            return NULL;
        }
    } else {
        vec->values = malloc((size_t)len * sizeof(uint64_t));
        if (vec->values == NULL) {
            free(vec);
            return NULL;
        }
        for (index = 0; index < vec->len; index += 1) {
            vec->values[index] = fill;
        }
    }
    return vec;
}

void* sarif_list_new_f64(int64_t len, double fill) {
    SarifList* vec = NULL;
    uint64_t index = 0;
    if (len < 0) {
        return NULL;
    }
    if ((uint64_t)len > (uint64_t)SIZE_MAX / sizeof(double)) {
        return NULL;
    }
    vec = calloc(1u, sizeof(SarifList));
    if (vec == NULL) {
        return NULL;
    }
    vec->len = (uint64_t)len;
    if (fill == 0.0) {
        vec->values = calloc((size_t)len, sizeof(double));
        if (vec->values == NULL) {
            free(vec);
            return NULL;
        }
    } else {
        double* fvalues = malloc((size_t)len * sizeof(double));
        if (fvalues == NULL) {
            free(vec);
            return NULL;
        }
        for (index = 0; index < (uint64_t)len; index++) {
            fvalues[index] = fill;
        }
        vec->values = (uint64_t*)fvalues;
    }
    return vec;
}
// =============================================================================
// Ordered Map[T, V] substrate using lightweight open-addressing with linear probing
// Maintains insertion order for ordered iteration (needed for csvgroupby, joinagg, sortuniq)
// =============================================================================

typedef struct SarifMapEntry {
    uint64_t key;
    uint64_t value;
    uint32_t hash;
    uint8_t occupied;
} SarifMapEntry;

typedef struct SarifMap {
    uint64_t len;
    uint64_t cap;
    SarifMapEntry* entries;
} SarifMap;

static uint32_t sarif_map_hash(uint64_t key) {
    // Simple hash mixing - good enough for our use case
    uint32_t x = (uint32_t)(key >> 32) ^ (uint32_t)(key & 0xffffffff);
    x = ((x >> 16) ^ x) * 0x45d9f3b;
    x = ((x >> 16) ^ x) * 0x45d9f3b;
    x = (x >> 16) ^ x;
    return x;
}

void* sarif_map_new(void) {
    SarifMap* map = calloc(1u, sizeof(SarifMap));
    if (map == NULL) {
        return NULL;
    }
    map->cap = 16;  // Start with reasonable capacity
    map->entries = calloc(map->cap, sizeof(SarifMapEntry));
    if (map->entries == NULL) {
        free(map);
        return NULL;
    }
    return map;
}

void* sarif_map_insert(void* map_ptr, uint64_t key, uint64_t value) {
    SarifMap* map = (SarifMap*)map_ptr;
    if (map == NULL || map->entries == NULL) {
        return NULL;
    }
    // Grow if load factor > 0.75
    if (map->len * 4 >= map->cap * 3) {
        uint64_t new_cap = map->cap * 2;
        SarifMapEntry* new_entries = realloc(map->entries, new_cap * sizeof(SarifMapEntry));
        if (new_entries == NULL) {
            return NULL;
        }
        // Rehash all entries
        memset(new_entries + map->cap, 0, (new_cap - map->cap) * sizeof(SarifMapEntry));
        for (uint64_t i = 0; i < map->cap; i++) {
            if (new_entries[i].occupied) {
                uint32_t new_idx = sarif_map_hash(new_entries[i].key) % new_cap;
                if (new_idx != i) {
                    SarifMapEntry tmp = new_entries[i];
                    new_entries[i].occupied = 0;
                    while (new_entries[new_idx].occupied) {
                        new_idx = (new_idx + 1) % new_cap;
                    }
                    new_entries[new_idx] = tmp;
                }
            }
        }
        map->entries = new_entries;
        map->cap = new_cap;
    }
    // Insert with linear probing
    uint32_t hash = sarif_map_hash(key);
    uint64_t idx = hash % map->cap;
    while (map->entries[idx].occupied) {
        if (map->entries[idx].key == key) {
            // Update existing
            map->entries[idx].value = value;
            return map;
        }
        idx = (idx + 1) % map->cap;
    }
    map->entries[idx].key = key;
    map->entries[idx].value = value;
    map->entries[idx].hash = hash;
    map->entries[idx].occupied = 1;
    map->len++;
    return map;
}

uint64_t sarif_map_get(void* map_ptr, uint64_t key, uint64_t default_val) {
    SarifMap* map = (SarifMap*)map_ptr;
    if (map == NULL || map->entries == NULL) {
        return default_val;
    }
    uint32_t hash = sarif_map_hash(key);
    uint64_t idx = hash % map->cap;
    uint64_t start = idx;
    while (map->entries[idx].occupied) {
        if (map->entries[idx].key == key) {
            return map->entries[idx].value;
        }
        idx = (idx + 1) % map->cap;
        if (idx == start) {
            break;
        }
    }
    return default_val;
}

void* sarif_text_concat(const unsigned char* left, const unsigned char* right) {
    uint64_t left_len = 0;
    uint64_t right_len = 0;
    uint64_t total_len = 0;
    size_t total_size = 0;
    unsigned char* text = NULL;
    if (left == NULL || right == NULL) {
        return NULL;
    }
    left_len = sarif_load_u64(left, 0);
    right_len = sarif_load_u64(right, 0);
    if (left_len == 0) {
        return (void*)right;
    }
    if (right_len == 0) {
        return (void*)left;
    }
    if (left_len > UINT64_MAX - right_len) {
        return NULL;
    }
    total_len = left_len + right_len;
    if (total_len > (uint64_t)SIZE_MAX - 8u) {
        return NULL;
    }
    total_size = (size_t)(8u + total_len);
    text = malloc(total_size);
    if (text == NULL) {
        return NULL;
    }
    sarif_store_u64(text, 0, total_len);
    if (left_len != 0) {
        memcpy(text + 8, left + 8, (size_t)left_len);
    }
    if (right_len != 0) {
        memcpy(text + 8 + left_len, right + 8, (size_t)right_len);
    }
    return text;
}

uint64_t sarif_text_eq(const unsigned char* left, const unsigned char* right) {
    uint64_t left_len = 0;
    uint64_t right_len = 0;
    if (left == right) {
        return 1;
    }
    if (left == NULL || right == NULL) {
        return 0;
    }
    left_len = sarif_load_u64(left, 0);
    right_len = sarif_load_u64(right, 0);
    if (left_len != right_len) {
        return 0;
    }
    if (left_len == 0) {
        return 1;
    }
    return memcmp(left + 8, right + 8, (size_t)left_len) == 0 ? 1 : 0;
}

int64_t sarif_text_cmp(const unsigned char* left, const unsigned char* right) {
    uint64_t left_len = 0;
    uint64_t right_len = 0;
    uint64_t shared_len = 0;
    int cmp = 0;
    if (left == right) {
        return 0;
    }
    if (left == NULL) {
        return right == NULL ? 0 : -1;
    }
    if (right == NULL) {
        return 1;
    }
    left_len = sarif_load_u64(left, 0);
    right_len = sarif_load_u64(right, 0);
    shared_len = left_len < right_len ? left_len : right_len;
    if (shared_len != 0) {
        cmp = memcmp(left + 8, right + 8, (size_t)shared_len);
        if (cmp < 0) {
            return -1;
        }
        if (cmp > 0) {
            return 1;
        }
    }
    if (left_len < right_len) {
        return -1;
    }
    if (left_len > right_len) {
        return 1;
    }
    return 0;
}

uint64_t sarif_text_eq_range(
    const unsigned char* source,
    int64_t start,
    int64_t end,
    const unsigned char* expected
) {
    uint64_t source_len = 0;
    uint64_t expected_len = 0;
    uint64_t clamped_start = 0;
    uint64_t clamped_end = 0;
    if (source == NULL || expected == NULL) {
        return 0;
    }
    source_len = sarif_load_u64(source, 0);
    expected_len = sarif_load_u64(expected, 0);
    if (start <= 0) {
        clamped_start = 0;
    } else {
        clamped_start = (uint64_t)start;
        if (clamped_start > source_len) {
            clamped_start = source_len;
        }
    }
    if (end <= 0) {
        clamped_end = 0;
    } else {
        clamped_end = (uint64_t)end;
        if (clamped_end > source_len) {
            clamped_end = source_len;
        }
    }
    while (clamped_start < source_len && sarif_is_utf8_continuation(source[8 + clamped_start])) {
        clamped_start += 1;
    }
    while (clamped_end < source_len && sarif_is_utf8_continuation(source[8 + clamped_end])) {
        clamped_end -= 1;
    }
    if (clamped_end < clamped_start) {
        clamped_end = clamped_start;
    }
    if (clamped_end - clamped_start != expected_len) {
        return 0;
    }
    if (expected_len == 0) {
        return 1;
    }
    return memcmp(source + 8 + clamped_start, expected + 8, (size_t)expected_len) == 0 ? 1 : 0;
}

void* sarif_text_slice(const unsigned char* text, uint64_t start, uint64_t end) {
    uint64_t len = 0;
    uint64_t clamped_start = 0;
    uint64_t clamped_end = 0;
    size_t slice_len = 0;
    unsigned char* result = NULL;
    if (text == NULL) {
        return NULL;
    }
    len = sarif_load_u64(text, 0);
    clamped_start = start < len ? start : len;
    clamped_end = end < len ? end : len;
    while (clamped_start < len && sarif_is_utf8_continuation(text[8 + clamped_start])) {
        clamped_start += 1;
    }
    while (clamped_end < len && sarif_is_utf8_continuation(text[8 + clamped_end])) {
        clamped_end -= 1;
    }
    if (clamped_end <= clamped_start) {
        return sarif_empty_text;
    }
    if (clamped_start == 0 && clamped_end == len) {
        return (void*)text;
    }
    slice_len = (size_t)(clamped_end - clamped_start);
    result = malloc(8u + slice_len);
    if (result == NULL) {
        return NULL;
    }
    sarif_store_u64(result, 0, (uint64_t)slice_len);
    memcpy(result + 8, text + 8 + clamped_start, slice_len);
    return result;
}

void* sarif_text_from_f64_fixed(double value, int64_t digits) {
    int precision = 0;
    int len = 0;
    unsigned char* result = NULL;
    if (digits > 0) {
        precision = digits > 1000 ? 1000 : (int)digits;
    }
    len = snprintf(NULL, 0, "%.*f", precision, value);
    if (len < 0 || (uint64_t)len > (uint64_t)SIZE_MAX - 8u) {
        return NULL;
    }
    result = malloc(8u + (size_t)len);
    if (result == NULL) {
        return NULL;
    }
    sarif_store_u64(result, 0, (uint64_t)len);
    if (len != 0) {
        snprintf((char*)(result + 8), (size_t)len + 1u, "%.*f", precision, value);
    }
    return result;
}

int64_t sarif_parse_i32(const unsigned char* text) {
    uint64_t len = 0;
    const unsigned char* bytes = NULL;
    uint64_t index = 0;
    uint64_t limit = 0;
    int negative = 0;
    int64_t value = 0;
    if (text == NULL) {
        return 0;
    }
    len = sarif_load_u64(text, 0);
    bytes = text + 8;
    if (len == 0) {
        return 0;
    }
    if (bytes[0] == '-') {
        negative = 1;
        index = 1;
        limit = (uint64_t)INT32_MAX + 1u;
    } else {
        limit = (uint64_t)INT32_MAX;
    }
    if (index == len) {
        return 0;
    }
    while (index < len) {
        uint64_t digit = 0;
        uint64_t next = 0;
        if (bytes[index] < '0' || bytes[index] > '9') {
            return 0;
        }
        digit = (uint64_t)(bytes[index] - '0');
        if ((uint64_t)value > limit / 10u) {
            return 0;
        }
        next = (uint64_t)value * 10u + digit;
        if (next > limit) {
            return 0;
        }
        value = (int64_t)next;
        index += 1;
    }
    if (negative) {
        return -value;
    }
    return value;
}

int64_t sarif_parse_i32_range(const unsigned char* text, int64_t start, int64_t end) {
    uint64_t len = 0;
    uint64_t index = 0;
    uint64_t limit = 0;
    int negative = 0;
    int64_t value = 0;
    const unsigned char* bytes = NULL;
    if (text == NULL) {
        return 0;
    }
    len = sarif_load_u64(text, 0);
    if (start <= 0) {
        index = 0;
    } else {
        index = (uint64_t)start;
        if (index > len) {
            index = len;
        }
    }
    if (end <= 0) {
        len = 0;
    } else {
        uint64_t clamped_end = (uint64_t)end;
        if (clamped_end < len) {
            len = clamped_end;
        }
    }
    bytes = text + 8;
    while (index < len && bytes[index] == ' ') {
        index += 1;
    }
    while (len > index && bytes[len - 1] == ' ') {
        len -= 1;
    }
    if (index == len) {
        return 0;
    }
    if (bytes[index] == '-') {
        negative = 1;
        index += 1;
        limit = (uint64_t)INT32_MAX + 1u;
    } else {
        limit = (uint64_t)INT32_MAX;
    }
    if (index == len) {
        return 0;
    }
    while (index < len) {
        uint64_t digit = 0;
        uint64_t next = 0;
        if (bytes[index] < '0' || bytes[index] > '9') {
            return 0;
        }
        digit = (uint64_t)(bytes[index] - '0');
        if ((uint64_t)value > limit / 10u) {
            return 0;
        }
        next = (uint64_t)value * 10u + digit;
        if (next > limit) {
            return 0;
        }
        value = (int64_t)next;
        index += 1;
    }
    if (negative) {
        return -value;
    }
    return value;
}

double sarif_parse_f64(const unsigned char* text) {
    uint64_t len = 0;
    char stack_buffer[128];
    char* heap_buffer = NULL;
    char* buffer = stack_buffer;
    char* end = NULL;
    double value = 0.0;
    if (text == NULL) {
        return 0.0;
    }
    len = sarif_load_u64(text, 0);
    if (len > (uint64_t)SIZE_MAX - 1u) {
        return 0.0;
    }
    if (len + 1u > sizeof(stack_buffer)) {
        heap_buffer = malloc((size_t)len + 1u);
        if (heap_buffer == NULL) {
            return 0.0;
        }
        buffer = heap_buffer;
    }
    if (len != 0) {
        memcpy(buffer, text + 8, (size_t)len);
    }
    buffer[len] = '\0';
    errno = 0;
    value = strtod(buffer, &end);
    if (end == buffer || *end != '\0' || errno != 0) {
        free(heap_buffer);
        return 0.0;
    }
    free(heap_buffer);
    return value;
}

uint64_t sarif_arg_count(void) {
    return sarif_argc < 0 ? 0u : (uint64_t)sarif_argc;
}

void* sarif_arg_text(int64_t index) {
    const char* value = "";
    size_t len = 0;
    unsigned char* result = NULL;
    if (index >= 0 && sarif_argv != NULL && index < sarif_argc) {
        value = sarif_argv[index];
    }
    len = strlen(value);
    result = malloc(8u + len);
    if (result == NULL) {
        return NULL;
    }
    sarif_store_u64(result, 0, (uint64_t)len);
    if (len != 0) {
        memcpy(result + 8, value, len);
    }
    return result;
}

void* sarif_stdin_text(void) {
    unsigned char* buffer = NULL;
    size_t len = 0;
    size_t cap = 0;
    unsigned char chunk[SARIF_STDIN_CHUNK_SIZE];
    size_t read = 0;

    if (sarif_stdin_cache != NULL) {
        return sarif_stdin_cache;
    }

    while ((read = fread(chunk, 1u, sizeof(chunk), stdin)) != 0u) {
        if (read > SIZE_MAX - len) {
            free(buffer);
            return NULL;
        }
        if (len + read > cap) {
            size_t next_cap = cap == 0 ? SARIF_STDIN_CHUNK_SIZE : cap;
            while (next_cap < len + read) {
                if (next_cap > SIZE_MAX / 2u) {
                    next_cap = len + read;
                    break;
                }
                next_cap *= 2u;
            }
            unsigned char* next = realloc(buffer, next_cap);
            if (next == NULL) {
                free(buffer);
                return NULL;
            }
            buffer = next;
            cap = next_cap;
        }
        memcpy(buffer + len, chunk, read);
        len += read;
    }
    if (ferror(stdin)) {
        free(buffer);
        return NULL;
    }

    sarif_stdin_cache = malloc(8u + len);
    if (sarif_stdin_cache == NULL) {
        free(buffer);
        return NULL;
    }
    sarif_store_u64(sarif_stdin_cache, 0, (uint64_t)len);
    if (len != 0) {
        memcpy(sarif_stdin_cache + 8, buffer, len);
    }
    free(buffer);
    return sarif_stdin_cache;
}

void sarif_stdout_write(const unsigned char* text) {
    (void)sarif_write_text_blob(text, 0);
}

static int sarif_write_text_blob(const unsigned char* text, int newline) {
    uint64_t len = 0;
    const unsigned char* bytes = NULL;
    if (text == NULL) {
        return 1;
    }
    len = sarif_load_u64(text, 0);
    bytes = text + 8;
    if (fwrite(bytes, 1, (size_t)len, stdout) != (size_t)len) {
        return 1;
    }
    if (newline && fputc('\n', stdout) == EOF) {
        return 1;
    }
    return 0;
}

static int sarif_write_value(
    uint32_t kind,
    uint64_t raw,
    const SarifRecordDesc* record,
    const SarifEnumDesc* enum_desc
);

static int sarif_enum_has_payloads(const SarifEnumDesc* enum_desc) {
    uint64_t index = 0;
    if (enum_desc == NULL) {
        return 0;
    }
    for (index = 0; index < enum_desc->variant_count; index += 1) {
        if (enum_desc->variants[index].payload_kind != 0) {
            return 1;
        }
    }
    return 0;
}

static int sarif_write_enum(uint64_t raw, const SarifEnumDesc* enum_desc) {
    uint64_t tag = raw;
    uint64_t payload = 0;
    const SarifVariantDesc* variant = NULL;
    const unsigned char* enum_ptr = NULL;
    if (enum_desc == NULL) {
        return 1;
    }
    if (sarif_enum_has_payloads(enum_desc)) {
        enum_ptr = (const unsigned char*)(uintptr_t)raw;
        if (enum_ptr == NULL) {
            return 1;
        }
        tag = sarif_load_u64(enum_ptr, 0);
        payload = sarif_load_u64(enum_ptr, 8);
    }
    if (tag >= enum_desc->variant_count) {
        return 1;
    }
    variant = &enum_desc->variants[tag];
    if (fputs(enum_desc->name, stdout) == EOF) {
        return 1;
    }
    if (fputc('.', stdout) == EOF) {
        return 1;
    }
    if (fputs(variant->name, stdout) == EOF) {
        return 1;
    }
    if (variant->payload_kind == 0) {
        return 0;
    }
    if (fputc('(', stdout) == EOF) {
        return 1;
    }
    if (sarif_write_value(variant->payload_kind, payload, variant->record, variant->enum_desc) != 0) {
        return 1;
    }
    return fputc(')', stdout) == EOF ? 1 : 0;
}

static int sarif_write_record(const unsigned char* record_ptr, const SarifRecordDesc* record) {
    uint64_t index = 0;
    if (record_ptr == NULL || record == NULL) {
        return 1;
    }
    if (fputs(record->name, stdout) == EOF) {
        return 1;
    }
    if (fputc('{', stdout) == EOF) {
        return 1;
    }
    for (index = 0; index < record->field_count; index += 1) {
        const SarifFieldDesc* field = &record->fields[index];
        const uint64_t raw = sarif_load_u64(record_ptr, field->offset);
        if (index != 0) {
            if (fputs(", ", stdout) == EOF) {
                return 1;
            }
        }
        if (fputs(field->name, stdout) == EOF) {
            return 1;
        }
        if (fputs(": ", stdout) == EOF) {
            return 1;
        }
        if (sarif_write_value(field->kind, raw, field->record, field->enum_desc) != 0) {
            return 1;
        }
    }
    return fputc('}', stdout) == EOF ? 1 : 0;
}

static int sarif_write_value(
    uint32_t kind,
    uint64_t raw,
    const SarifRecordDesc* record,
    const SarifEnumDesc* enum_desc
) {
    switch (kind) {
        case 1:
            return fprintf(stdout, "%lld", (long long)(int64_t)raw) < 0 ? 1 : 0;
        case 2:
            return fputs(raw != 0 ? "true" : "false", stdout) == EOF ? 1 : 0;
        case 3:
            return sarif_write_text_blob((const unsigned char*)(uintptr_t)raw, 0);
        case 4:
            return sarif_write_record((const unsigned char*)(uintptr_t)raw, record);
        case 5:
            return sarif_write_enum(raw, enum_desc);
        case 6: {
            double value = 0.0;
            memcpy(&value, &raw, sizeof(value));
            return fprintf(stdout, "%.17g", value) < 0 ? 1 : 0;
        }
        default:
            return 1;
    }
}

int main(int argc, char** argv) {
    sarif_argc = argc;
    sarif_argv = argv;
#if SARIF_MAIN_KIND == 1
    int32_t value = sarif_user_main();
#if SARIF_MAIN_PRINT
    return fprintf(stdout, "%lld\n", (long long)value) < 0 ? 1 : 0;
#else
    return (int)value;
#endif
#elif SARIF_MAIN_KIND == 2
    uint32_t value = sarif_user_main();
#if SARIF_MAIN_PRINT
    if (fputs(value != 0 ? "true" : "false", stdout) == EOF) {
        return 1;
    }
    return fputc('\n', stdout) == EOF ? 1 : 0;
#else
    return value ? 0 : 1;
#endif
#elif SARIF_MAIN_KIND == 3
    const unsigned char* text = (const unsigned char*)(uintptr_t)sarif_user_main();
    return sarif_write_text_blob(text, 0);
#elif SARIF_MAIN_KIND == 4
    const unsigned char* record = (const unsigned char*)(uintptr_t)sarif_user_main();
    if (sarif_write_record(record, sarif_get_main_record_desc()) != 0) {
        return 1;
    }
    return fputc('\n', stdout) == EOF ? 1 : 0;
#elif SARIF_MAIN_KIND == 5
    if (sarif_write_enum(sarif_user_main(), sarif_get_main_enum_desc()) != 0) {
        return 1;
    }
    return fputc('\n', stdout) == EOF ? 1 : 0;
#elif SARIF_MAIN_KIND == 6
    double value = sarif_user_main();
#if SARIF_MAIN_PRINT
    return fprintf(stdout, "%.17g\n", value) < 0 ? 1 : 0;
#else
    (void)value;
    return 0;
#endif
#else
    sarif_user_main();
    return 0;
#endif
}
