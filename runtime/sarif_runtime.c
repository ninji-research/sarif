#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <errno.h>
#include <limits.h>
#include <string.h>
#include <unistd.h>

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
static int sarif_write_i64(int64_t value, int newline);
int64_t sarif_text_cmp(const unsigned char* left, const unsigned char* right);

static int sarif_write_all(const unsigned char* bytes, uint64_t len) {
    while (len != 0) {
        size_t chunk = len > (uint64_t)SIZE_MAX ? SIZE_MAX : (size_t)len;
        ssize_t written = write(STDOUT_FILENO, bytes, chunk);
        if (written <= 0) {
            return 1;
        }
        bytes += (size_t)written;
        len -= (uint64_t)written;
    }
    return 0;
}

static int sarif_write_byte(unsigned char byte) {
    return sarif_write_all(&byte, 1);
}

#define SARIF_RECORD_ALIGN 16u
#define SARIF_RECORD_ARENA_CHUNK_MIN_SIZE (1u << 14)
#define SARIF_RECORD_ARENA_CHUNK_MAX_SIZE (1u << 20)
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

// SarifList stores opaque 64-bit slots; typed interpretation happens at the
// call boundary so the runtime keeps one list representation.
struct SarifList {
    uint64_t len;
    uint64_t* values;  // elements stored as bitcast handles
};

#if SARIF_MAIN_KIND == 4
extern const SarifRecordDesc* sarif_get_main_record_desc(void);
#elif SARIF_MAIN_KIND == 5
extern const SarifEnumDesc* sarif_get_main_enum_desc(void);
#endif

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

static size_t sarif_record_next_chunk_cap(size_t aligned) {
    size_t target = SARIF_RECORD_ARENA_CHUNK_MIN_SIZE;
    if (sarif_record_current != NULL && sarif_record_current->cap > target) {
        target = sarif_record_current->cap;
        if (target < SARIF_RECORD_ARENA_CHUNK_MAX_SIZE / 2u) {
            target *= 2u;
        } else {
            target = SARIF_RECORD_ARENA_CHUNK_MAX_SIZE;
        }
    }
    if (target < aligned) {
        target = aligned;
    }
    return target;
}

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
    min_cap = sarif_record_next_chunk_cap(aligned);
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

static inline __attribute__((always_inline)) void sarif_store_u64(unsigned char* base, uint64_t offset, uint64_t value) {
    memcpy(base + offset, &value, sizeof(uint64_t));
}

static inline __attribute__((always_inline)) uint64_t sarif_load_u64(const unsigned char* base, uint64_t offset) {
    uint64_t value;
    memcpy(&value, base + offset, sizeof(uint64_t));
    return value;
}

static int sarif_is_utf8_continuation(unsigned char byte) {
    return (byte & 0xc0u) == 0x80u;
}

static void sarif_clamp_text_range(const unsigned char* source, uint64_t len, int64_t* start, int64_t* end) {
    if (*start <= 0) {
        *start = 0;
    } else if ((uint64_t)*start > len) {
        *start = (int64_t)len;
    }
    if (*end <= 0) {
        *end = 0;
    } else if ((uint64_t)*end > len) {
        *end = (int64_t)len;
    }
    while (*start < (int64_t)len && sarif_is_utf8_continuation(source[8 + *start])) {
        (*start)++;
    }
    while (*end < (int64_t)len && sarif_is_utf8_continuation(source[8 + *end])) {
        (*end)--;
    }
    if (*end < *start) {
        *end = *start;
    }
}

void* sarif_text_builder_new(void) {
    SarifTextBuilder* builder = malloc(sizeof(SarifTextBuilder));
    if (builder == NULL) {
        return NULL;
    }
    builder->len = 0;
    builder->cap = 0;
    builder->bytes = NULL;
    return builder;
}

static inline __attribute__((always_inline)) SarifTextBuilder* sarif_text_builder_reserve(
    SarifTextBuilder* builder,
    uint64_t appended_len
) {
    uint64_t required = 0;
    uint64_t next_cap = 0;
    unsigned char* grown = NULL;
    if (builder == NULL || appended_len == 0) {
        return builder;
    }
    if (builder->len > UINT64_MAX - appended_len) {
        return NULL;
    }
    required = builder->len + appended_len;
    if (required <= builder->cap) {
        return builder;
    }
    next_cap = builder->cap;
    if (next_cap == 0) {
        next_cap = required;
    } else {
        while (next_cap < required) {
            next_cap += next_cap / 2u + 1u;
        }
    }
    if (next_cap > (uint64_t)SIZE_MAX) {
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

__attribute__((always_inline)) void* sarif_text_builder_append_ascii(void* raw_builder, int64_t byte) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    if (builder == NULL || byte < 0 || byte > 0x7f) {
        return NULL;
    }
    builder = sarif_text_builder_reserve(builder, 1);
    if (builder == NULL) {
        return NULL;
    }
    builder->bytes[builder->len] = (unsigned char)byte;
    builder->len += 1;
    return builder;
}

void* sarif_text_builder_append_slice(
    void* raw_builder,
    const unsigned char* text,
    int64_t start,
    int64_t end
) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    uint64_t text_len = 0;
    uint64_t slice_len = 0;
    if (builder == NULL || text == NULL || start < 0 || end < start) {
        return NULL;
    }
    text_len = sarif_load_u64(text, 0);
    if ((uint64_t)end > text_len) {
        return NULL;
    }
    slice_len = (uint64_t)(end - start);
    if (slice_len == 0) {
        return builder;
    }
    builder = sarif_text_builder_reserve(builder, slice_len);
    if (builder == NULL) {
        return NULL;
    }
    memcpy(builder->bytes + builder->len, text + 8 + start, (size_t)slice_len);
    builder->len += slice_len;
    return builder;
}

static int sarif_format_i64(char* scratch, int64_t value) {
    int index = 20;
    uint64_t magnitude;
    if (value < 0) {
        scratch[--index] = (char)('0' + (-(value % 10)));
        magnitude = (uint64_t)(-(value / 10));
        while (magnitude != 0) {
            scratch[--index] = (char)('0' + (magnitude % 10));
            magnitude /= 10;
        }
        scratch[--index] = '-';
    } else {
        magnitude = (uint64_t)value;
        do {
            scratch[--index] = (char)('0' + (magnitude % 10));
            magnitude /= 10;
        } while (magnitude != 0);
    }
    return 20 - index;
}

void* sarif_text_builder_append_i32(void* raw_builder, int64_t value) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    char scratch[21];
    int len;
    if (builder == NULL) {
        return NULL;
    }
    len = sarif_format_i64(scratch, value);
    builder = sarif_text_builder_reserve(builder, (uint64_t)len);
    if (builder == NULL) {
        return NULL;
    }
    memcpy(builder->bytes + builder->len, scratch + (20 - len), (size_t)len);
    builder->len += (uint64_t)len;
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
    if (len < 0 || (uint64_t)len > (uint64_t)SIZE_MAX / sizeof(uint64_t)) {
        return NULL;
    }
    vec = malloc(sizeof(SarifList));
    if (vec == NULL) {
        return NULL;
    }
    vec->len = (uint64_t)len;
    if ((size_t)len == 0) {
        vec->values = NULL;
    } else if (fill == 0) {
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

void* sarif_list_push(void* list_ptr, int64_t len, uint64_t value) {
    SarifList* list = (SarifList*)list_ptr;
    uint64_t used = 0;
    uint64_t next_cap = 0;
    uint64_t* grown = NULL;
    if (list == NULL || list->values == NULL || len < 0) {
        return NULL;
    }
    used = (uint64_t)len;
    if (used < list->len) {
        list->values[used] = value;
        return list;
    }
    if (used != list->len) {
        return NULL;
    }
    next_cap = used == 0 ? 8u : used * 2u;
    if (next_cap <= used) {
        next_cap = used + 1u;
    }
    if (next_cap > (uint64_t)SIZE_MAX / sizeof(uint64_t)) {
        return NULL;
    }
    grown = realloc(list->values, (size_t)next_cap * sizeof(uint64_t));
    if (grown == NULL) {
        return NULL;
    }
    list->values = grown;
    list->values[used] = value;
    list->len = next_cap;
    return list;
}

static int sarif_compare_text_handles(uint64_t left, uint64_t right) {
    return (int)sarif_text_cmp((const unsigned char*)left, (const unsigned char*)right);
}

static int sarif_compare_record_text_field_handles(uint64_t left, uint64_t right, uint64_t offset) {
    const unsigned char* left_record = (const unsigned char*)left;
    const unsigned char* right_record = (const unsigned char*)right;
    uint64_t left_text = 0;
    uint64_t right_text = 0;
    if (left_record == right_record) {
        return 0;
    }
    if (left_record == NULL) {
        return right_record == NULL ? 0 : -1;
    }
    if (right_record == NULL) {
        return 1;
    }
    left_text = sarif_load_u64(left_record, offset);
    right_text = sarif_load_u64(right_record, offset);
    return sarif_compare_text_handles(left_text, right_text);
}

static uint64_t sarif_sort_text_field_offset = 0;

static int sarif_qsort_compare_text_handles(const void* left, const void* right) {
    const uint64_t left_handle = *(const uint64_t*)left;
    const uint64_t right_handle = *(const uint64_t*)right;
    return sarif_compare_text_handles(left_handle, right_handle);
}

static int sarif_qsort_compare_record_text_field_handles(const void* left, const void* right) {
    const uint64_t left_handle = *(const uint64_t*)left;
    const uint64_t right_handle = *(const uint64_t*)right;
    return sarif_compare_record_text_field_handles(
        left_handle,
        right_handle,
        sarif_sort_text_field_offset
    );
}

void* sarif_list_sort_text(void* list_ptr, int64_t len) {
    SarifList* list = (SarifList*)list_ptr;
    uint64_t used = 0;
    if (list == NULL || list->values == NULL || len < 0) {
        return NULL;
    }
    used = (uint64_t)len;
    if (used > list->len) {
        return NULL;
    }
    if (used > 1) {
        qsort(
            list->values,
            (size_t)used,
            sizeof(uint64_t),
            sarif_qsort_compare_text_handles
        );
    }
    return list;
}

void* sarif_list_sort_by_text_field(void* list_ptr, int64_t len, int64_t offset) {
    SarifList* list = (SarifList*)list_ptr;
    uint64_t used = 0;
    uint64_t field_offset = 0;
    if (list == NULL || list->values == NULL || len < 0 || offset < 0) {
        return NULL;
    }
    used = (uint64_t)len;
    field_offset = (uint64_t)offset;
    if (used > list->len) {
        return NULL;
    }
    if (used > 1) {
        sarif_sort_text_field_offset = field_offset;
        qsort(
            list->values,
            (size_t)used,
            sizeof(uint64_t),
            sarif_qsort_compare_record_text_field_handles
        );
    }
    return list;
}
// =============================================================================
// TextIndex substrate: content-aware Text -> I32 open-addressed index.
// This is the maintained native primitive for text-keyed aggregation.
// =============================================================================

typedef struct SarifTextIndexEntry {
    uint64_t key;
    int64_t value;
    uint32_t hash;
    uint8_t occupied;
} SarifTextIndexEntry;

typedef struct SarifTextIndex {
    uint64_t len;
    uint64_t cap;
    SarifTextIndexEntry* entries;
} SarifTextIndex;

static int sarif_text_handle_eq(uint64_t left, uint64_t right);

static int sarif_text_index_ensure_capacity(SarifTextIndex* index) {
    if (index == NULL || index->entries == NULL) {
        return 0;
    }
    if (index->len * 4 < index->cap * 3) {
        return 1;
    }
    uint64_t new_cap = index->cap * 2;
    SarifTextIndexEntry* new_entries = calloc((size_t)new_cap, sizeof(SarifTextIndexEntry));
    if (new_entries == NULL) {
        return 0;
    }
    for (uint64_t i = 0; i < index->cap; i += 1) {
        if (index->entries[i].occupied) {
            uint64_t idx = index->entries[i].hash % new_cap;
            while (new_entries[idx].occupied) {
                idx = (idx + 1) % new_cap;
            }
            new_entries[idx] = index->entries[i];
        }
    }
    free(index->entries);
    index->entries = new_entries;
    index->cap = new_cap;
    return 1;
}

static SarifTextIndexEntry* sarif_text_index_find_entry(
    SarifTextIndex* index,
    uint64_t key,
    uint32_t hash,
    int* found
) {
    uint64_t idx = 0;
    uint64_t start = 0;
    if (found != NULL) {
        *found = 0;
    }
    if (index == NULL || index->entries == NULL) {
        return NULL;
    }
    idx = hash % index->cap;
    start = idx;
    while (index->entries[idx].occupied) {
        if (
            index->entries[idx].hash == hash &&
            sarif_text_handle_eq(index->entries[idx].key, key)
        ) {
            if (found != NULL) {
                *found = 1;
            }
            return &index->entries[idx];
        }
        idx = (idx + 1) % index->cap;
        if (idx == start) {
            return NULL;
        }
    }
    return &index->entries[idx];
}

static uint32_t sarif_text_hash_handle(uint64_t key) {
    const unsigned char* text = (const unsigned char*)key;
    uint64_t len = 0;
    uint32_t hash = 2166136261u;
    uint64_t i = 0;
    if (text == NULL) {
        return 0u;
    }
    len = sarif_load_u64(text, 0);
    for (i = 0; i < len; i += 1) {
        hash ^= text[8 + i];
        hash *= 16777619u;
    }
    hash ^= (uint32_t)len;
    return hash;
}

static inline __attribute__((always_inline)) int sarif_text_handle_eq(uint64_t left, uint64_t right) {
    const unsigned char* left_text = (const unsigned char*)left;
    const unsigned char* right_text = (const unsigned char*)right;
    uint64_t left_len = 0;
    uint64_t right_len = 0;
    if (left_text == right_text) {
        return 1;
    }
    if (left_text == NULL || right_text == NULL) {
        return 0;
    }
    left_len = sarif_load_u64(left_text, 0);
    right_len = sarif_load_u64(right_text, 0);
    if (left_len != right_len) {
        return 0;
    }
    if (left_len == 0) {
        return 1;
    }
    return memcmp(left_text + 8, right_text + 8, (size_t)left_len) == 0 ? 1 : 0;
}

void* sarif_text_index_new(void) {
    SarifTextIndex* index = malloc(sizeof(SarifTextIndex));
    if (index == NULL) {
        return NULL;
    }
    index->len = 0;
    index->cap = 16;
    index->entries = calloc(index->cap, sizeof(SarifTextIndexEntry));
    if (index->entries == NULL) {
        free(index);
        return NULL;
    }
    return index;
}

void* sarif_text_index_set(void* index_ptr, uint64_t key, int64_t value) {
    SarifTextIndex* index = (SarifTextIndex*)index_ptr;
    uint32_t hash = 0;
    int found = 0;
    SarifTextIndexEntry* entry = NULL;
    if (index == NULL || index->entries == NULL) {
        return NULL;
    }
    if (!sarif_text_index_ensure_capacity(index)) {
        return NULL;
    }
    hash = sarif_text_hash_handle(key);
    entry = sarif_text_index_find_entry(index, key, hash, &found);
    if (entry == NULL) {
        return NULL;
    }
    entry->key = key;
    entry->value = value;
    entry->hash = hash;
    if (!found) {
        entry->occupied = 1;
        index->len += 1;
    }
    return index;
}

int64_t sarif_text_index_get(void* index_ptr, uint64_t key) {
    SarifTextIndex* index = (SarifTextIndex*)index_ptr;
    int found = 0;
    SarifTextIndexEntry* entry = sarif_text_index_find_entry(
        index,
        key,
        sarif_text_hash_handle(key),
        &found
    );
    if (entry != NULL && found) {
        return entry->value;
    }
    return -1;
}

int64_t sarif_text_index_get_or_insert(void* index_ptr, uint64_t key, int64_t next) {
    SarifTextIndex* index = (SarifTextIndex*)index_ptr;
    int found = 0;
    uint32_t hash = 0;
    SarifTextIndexEntry* entry = NULL;
    if (index == NULL || index->entries == NULL) {
        return -1;
    }
    if (!sarif_text_index_ensure_capacity(index)) {
        return -1;
    }
    hash = sarif_text_hash_handle(key);
    entry = sarif_text_index_find_entry(index, key, hash, &found);
    if (entry == NULL) {
        return -1;
    }
    if (found) {
        return entry->value;
    }
    entry->key = key;
    entry->value = next;
    entry->hash = hash;
    entry->occupied = 1;
    index->len += 1;
    return next;
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
    return sarif_text_cmp(left, right) == 0 ? 1 : 0;
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
    if (source == NULL || expected == NULL) {
        return 0;
    }
    source_len = sarif_load_u64(source, 0);
    expected_len = sarif_load_u64(expected, 0);
    sarif_clamp_text_range(source, source_len, &start, &end);
    if ((uint64_t)(end - start) != expected_len) {
        return 0;
    }
    if (expected_len == 0) {
        return 1;
    }
    return memcmp(source + 8 + start, expected + 8, (size_t)expected_len) == 0 ? 1 : 0;
}

int64_t sarif_text_find_byte_range(
    const unsigned char* source,
    int64_t start,
    int64_t end,
    int64_t byte
) {
    uint64_t source_len = 0;
    const unsigned char* found = NULL;
    unsigned char needle = 0;
    if (source == NULL) {
        return end;
    }
    source_len = sarif_load_u64(source, 0);
    sarif_clamp_text_range(source, source_len, &start, &end);
    needle = (unsigned char)((uint64_t)byte & 0xffu);
    if (end == start) {
        return end;
    }
    found = memchr(source + 8 + start, needle, (size_t)(end - start));
    if (found != NULL) {
        return (int64_t)(found - (source + 8));
    }
    return end;
}

int64_t sarif_text_line_end(const unsigned char* source, int64_t start) {
    uint64_t source_len = 0;
    const unsigned char* found = NULL;
    uint64_t line_end = 0;
    if (source == NULL) {
        return 0;
    }
    source_len = sarif_load_u64(source, 0);
    if (start <= 0) {
        start = 0;
    } else if ((uint64_t)start > source_len) {
        start = (int64_t)source_len;
    }
    while (start < (int64_t)source_len && sarif_is_utf8_continuation(source[8 + start])) {
        start++;
    }
    found = memchr(source + 8 + start, '\n', (size_t)(source_len - start));
    line_end = found == NULL ? source_len : (uint64_t)(found - (source + 8));
    if (line_end > (uint64_t)start && source[8 + line_end - 1] == '\r') {
        return (int64_t)(line_end - 1);
    }
    return (int64_t)line_end;
}

int64_t sarif_text_next_line(const unsigned char* source, int64_t start) {
    uint64_t source_len = 0;
    const unsigned char* found = NULL;
    if (source == NULL) {
        return 0;
    }
    source_len = sarif_load_u64(source, 0);
    if (start <= 0) {
        start = 0;
    } else if ((uint64_t)start > source_len) {
        start = (int64_t)source_len;
    }
    while (start < (int64_t)source_len && sarif_is_utf8_continuation(source[8 + start])) {
        start++;
    }
    found = memchr(source + 8 + start, '\n', (size_t)(source_len - start));
    if (found != NULL) {
        return (int64_t)(found - (source + 8)) + 1;
    }
    return (int64_t)source_len;
}

#define sarif_text_field_end(source, start, end, byte) \
    sarif_text_find_byte_range(source, start, end, byte)

int64_t sarif_text_next_field(
    const unsigned char* source,
    int64_t start,
    int64_t end,
    int64_t byte
) {
    int64_t field_end = sarif_text_find_byte_range(source, start, end, byte);
    uint64_t source_len = source ? sarif_load_u64(source, 0) : 0;
    if (field_end < end && field_end < (int64_t)source_len) {
        return field_end + 1;
    }
    return field_end;
}

static void* sarif_slice_blob(const unsigned char* blob, uint64_t start, uint64_t end, int utf8_aware) {
    uint64_t len = 0;
    uint64_t cs = 0, ce = 0;
    size_t slen = 0;
    unsigned char* result = NULL;
    if (blob == NULL) return NULL;
    len = sarif_load_u64(blob, 0);
    cs = start < len ? start : len;
    ce = end < len ? end : len;
    if (utf8_aware) {
        while (cs < len && sarif_is_utf8_continuation(blob[8 + cs])) cs++;
        while (ce < len && sarif_is_utf8_continuation(blob[8 + ce])) ce--;
        if (ce <= cs) return sarif_empty_text;
    } else {
        if (ce <= cs) return sarif_empty_text;
    }
    if (cs == 0 && ce == len) return (void*)blob;
    slen = (size_t)(ce - cs);
    result = malloc(8u + slen);
    if (!result) return NULL;
    sarif_store_u64(result, 0, (uint64_t)slen);
    memcpy(result + 8, blob + 8 + cs, slen);
    return result;
}

void* sarif_text_slice(const unsigned char* text, uint64_t start, uint64_t end) {
    return sarif_slice_blob(text, start, end, 1);
}

void* sarif_bytes_slice(const unsigned char* bytes, uint64_t start, uint64_t end) {
    return sarif_slice_blob(bytes, start, end, 0);
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
    uint64_t len, index;
    uint64_t limit;
    int negative = 0;
    int64_t value = 0;
    const unsigned char* bytes;
    if (text == NULL) return 0;
    len = sarif_load_u64(text, 0);
    index = start > 0 ? (uint64_t)start < len ? (uint64_t)start : len : 0;
    len = end > 0 ? (uint64_t)end < len ? (uint64_t)end : len : 0;
    bytes = text + 8;
    while (index < len && bytes[index] == ' ') index += 1;
    while (len > index && bytes[len - 1] == ' ') len -= 1;
    if (index == len) return 0;
    if (bytes[index] == '-') {
        negative = 1;
        index += 1;
        limit = (uint64_t)INT32_MAX + 1u;
    } else {
        limit = (uint64_t)INT32_MAX;
    }
    if (index == len) return 0;
    while (index < len) {
        uint64_t digit, next;
        if (bytes[index] < '0' || bytes[index] > '9') return 0;
        digit = (uint64_t)(bytes[index] - '0');
        if ((uint64_t)value > limit / 10u) return 0;
        next = (uint64_t)value * 10u + digit;
        if (next > limit) return 0;
        value = (int64_t)next;
        index += 1;
    }
    return negative ? -value : value;
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

void* sarif_stdout_write_builder(void* raw_builder) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    if (builder == NULL) {
        return NULL;
    }
    if (builder->len != 0 && sarif_write_all(builder->bytes, builder->len) != 0) {
        return NULL;
    }
    builder->len = 0;
    return builder;
}

static int sarif_write_text_blob(const unsigned char* text, int newline) {
    uint64_t len = 0;
    const unsigned char* bytes = NULL;
    if (text == NULL) {
        return 1;
    }
    len = sarif_load_u64(text, 0);
    bytes = text + 8;
    if (sarif_write_all(bytes, len) != 0) {
        return 1;
    }
    if (newline && sarif_write_byte('\n') != 0) {
        return 1;
    }
    return 0;
}

#if SARIF_MAIN_KIND == 4 || SARIF_MAIN_KIND == 5
static int sarif_write_value(
    uint32_t kind,
    uint64_t raw,
    const SarifRecordDesc* record,
    const SarifEnumDesc* enum_desc
);
#endif

static int sarif_write_i64(int64_t value, int newline) {
    char scratch[21];
    int len = sarif_format_i64(scratch, value);
    if (sarif_write_all((const unsigned char*)(scratch + (20 - len)), (uint64_t)len) != 0) {
        return 1;
    }
    if (newline && sarif_write_byte('\n') != 0) {
        return 1;
    }
    return 0;
}

#if SARIF_MAIN_KIND == 4 || SARIF_MAIN_KIND == 5
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
    if (sarif_write_all((const unsigned char*)enum_desc->name, (uint64_t)strlen(enum_desc->name)) != 0) {
        return 1;
    }
    if (sarif_write_byte('.') != 0) {
        return 1;
    }
    if (sarif_write_all((const unsigned char*)variant->name, (uint64_t)strlen(variant->name)) != 0) {
        return 1;
    }
    if (variant->payload_kind == 0) {
        return 0;
    }
    if (sarif_write_byte('(') != 0) {
        return 1;
    }
    if (sarif_write_value(variant->payload_kind, payload, variant->record, variant->enum_desc) != 0) {
        return 1;
    }
    return sarif_write_byte(')') != 0 ? 1 : 0;
}

static int sarif_write_record(const unsigned char* record_ptr, const SarifRecordDesc* record) {
    uint64_t index = 0;
    if (record_ptr == NULL || record == NULL) {
        return 1;
    }
    if (sarif_write_all((const unsigned char*)record->name, (uint64_t)strlen(record->name)) != 0) {
        return 1;
    }
    if (sarif_write_byte('{') != 0) {
        return 1;
    }
    for (index = 0; index < record->field_count; index += 1) {
        const SarifFieldDesc* field = &record->fields[index];
        const uint64_t raw = sarif_load_u64(record_ptr, field->offset);
        if (index != 0) {
            if (sarif_write_all((const unsigned char*)", ", 2) != 0) {
                return 1;
            }
        }
        if (sarif_write_all((const unsigned char*)field->name, (uint64_t)strlen(field->name)) != 0) {
            return 1;
        }
        if (sarif_write_all((const unsigned char*)": ", 2) != 0) {
            return 1;
        }
        if (sarif_write_value(field->kind, raw, field->record, field->enum_desc) != 0) {
            return 1;
        }
    }
    return sarif_write_byte('}') != 0 ? 1 : 0;
}

static int sarif_write_value(
    uint32_t kind,
    uint64_t raw,
    const SarifRecordDesc* record,
    const SarifEnumDesc* enum_desc
) {
    switch (kind) {
        case 1:
            return sarif_write_i64((int64_t)raw, 0);
        case 2:
            return sarif_write_all((const unsigned char*)(raw != 0 ? "true" : "false"), raw != 0 ? 4u : 5u);
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
#endif

int main(int argc, char** argv) {
    sarif_argc = argc;
    sarif_argv = argv;
#if SARIF_MAIN_KIND == 1
    int32_t value = sarif_user_main();
#if SARIF_MAIN_PRINT
    return sarif_write_i64((int64_t)value, 1);
#else
    return (int)value;
#endif
#elif SARIF_MAIN_KIND == 2
    uint32_t value = sarif_user_main();
#if SARIF_MAIN_PRINT
    if (sarif_write_all((const unsigned char*)(value != 0 ? "true" : "false"), value != 0 ? 4u : 5u) != 0) {
        return 1;
    }
    return sarif_write_byte('\n') != 0 ? 1 : 0;
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
    return sarif_write_byte('\n') != 0 ? 1 : 0;
#elif SARIF_MAIN_KIND == 5
    if (sarif_write_enum(sarif_user_main(), sarif_get_main_enum_desc()) != 0) {
        return 1;
    }
    return sarif_write_byte('\n') != 0 ? 1 : 0;
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
