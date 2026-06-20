#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <errno.h>
#include <limits.h>
#include <string.h>
#include <unistd.h>
#include <math.h>
#include <sys/types.h>
#include <dirent.h>
#include <time.h>
#include <pthread.h>

#ifndef SARIF_MAIN_KIND
#define SARIF_MAIN_KIND 0
#endif

#ifndef SARIF_MAIN_PRINT
#define SARIF_MAIN_PRINT 0
#endif

static int sarif_argc = 0;
static char** sarif_argv = NULL;
static unsigned char* sarif_stdin_cache = NULL;
static pthread_mutex_t sarif_env_mutex = PTHREAD_MUTEX_INITIALIZER;

__attribute__((noreturn)) static void sarif_fatal_error(const char* msg) {
    fprintf(stderr, "SARIF RUNTIME ERROR: %s\n", msg);
    exit(1);
}
static unsigned char sarif_empty_text[8] = {0};

#define SARIF_BYTES_VIEW_TAG (1ULL << 63)

#define UTF8_CONT_PREFIX_1BYTE 0x00u
#define UTF8_CONT_PREFIX_2BYTE 0xc0u
#define UTF8_CONT_PREFIX_3BYTE 0xe0u
#define UTF8_CONT_PREFIX_4BYTE 0xf0u
#define UTF8_CONTINUATION_MASK 0x80u
#define UTF8_DATA_MASK 0x3fu

/* Fast-path bounds for formatting integral doubles when precision == 0.
 * We restrict this optimization to a conservative ±1e12 range so integer
 * conversion remains efficient and predictable, while avoiding edge cases
 * and potential precision pitfalls at very large magnitudes.
 */
#define SARIF_F64_FIXED_FASTPATH_MIN (-1000000000000.0)
#define SARIF_F64_FIXED_FASTPATH_MAX (1000000000000.0)

static int sarif_should_use_integer_fastpath(double value, int precision) {
    int is_finite_value = isfinite(value);
    int is_in_fastpath_range = is_finite_value &&
        value >= SARIF_F64_FIXED_FASTPATH_MIN &&
        value <= SARIF_F64_FIXED_FASTPATH_MAX;
    int is_integral_value = is_in_fastpath_range &&
        value == (double)(int64_t)value;
    return precision == 0 && is_integral_value;
}

static int sarif_write_text_blob(const unsigned char* text, int newline);
static int __attribute__((unused)) sarif_write_i64(int64_t value, int newline);
int64_t sarif_text_cmp(const unsigned char* left, const unsigned char* right);

uint64_t sarif_text_len(const unsigned char* text);

static int sarif_write_all(const unsigned char* bytes, uint64_t len) {
    if (len == 0) {
        return 0;
    }
    size_t chunk = len > (uint64_t)SIZE_MAX ? SIZE_MAX : (size_t)len;
    if (fwrite(bytes, 1, chunk, stdout) != chunk) {
        return 1;
    }
    fflush(stdout);
    return 0;
}

static int sarif_write_byte(unsigned char byte) {
    return sarif_write_all(&byte, 1);
}

#define SARIF_RECORD_ALIGN 16u
#define SARIF_RECORD_ARENA_CHUNK_MIN_SIZE (1u << 14)
#define SARIF_RECORD_ARENA_CHUNK_MAX_SIZE (1u << 20)
#define SARIF_STDIN_CHUNK_SIZE 16384u
#define SARIF_TEXT_BUILDER_CHUNK 256u

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
};

struct SarifAllocScopeOverflow {
    struct SarifAllocScope scope;
    struct SarifAllocScopeOverflow* next;
};

#define SARIF_SCOPE_STACK_CAP 64u

static struct SarifAllocScope sarif_scope_stack[SARIF_SCOPE_STACK_CAP];
static uint64_t sarif_scope_depth = 0;
static struct SarifAllocScopeOverflow* sarif_scope_overflow = NULL;

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
static pthread_mutex_t sarif_record_mutex = PTHREAD_MUTEX_INITIALIZER;

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
    void* result = NULL;

    pthread_mutex_lock(&sarif_record_mutex);

    if (size == 0) {
        result = sarif_empty_text;
        goto done;
    }
    if (size > (uint64_t)SIZE_MAX) {
        goto done;
    }
    aligned = (size_t)size;
    if (aligned > SIZE_MAX - (SARIF_RECORD_ALIGN - 1u)) {
        goto done;
    }
    aligned = (aligned + (SARIF_RECORD_ALIGN - 1u)) & ~(SARIF_RECORD_ALIGN - 1u);
    chunk = sarif_record_current;
    if (chunk != NULL && aligned <= chunk->cap - chunk->used) {
        result = chunk->data + chunk->used;
        chunk->used += aligned;
        goto done;
    }
    min_cap = sarif_record_next_chunk_cap(aligned);
    if (min_cap > SIZE_MAX - sizeof(SarifRecordChunk)) {
        goto done;
    }
    chunk = malloc(sizeof(SarifRecordChunk) + min_cap);
    if (chunk == NULL) {
        goto done;
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
    result = chunk->data;

done:
    pthread_mutex_unlock(&sarif_record_mutex);
    return result;
}

static struct SarifAllocScope* sarif_alloc_push_scope(void) {
    if (sarif_scope_depth < SARIF_SCOPE_STACK_CAP) {
        return &sarif_scope_stack[sarif_scope_depth++];
    }
    struct SarifAllocScopeOverflow* n = malloc(sizeof(struct SarifAllocScopeOverflow));
    if (n == NULL) {
        sarif_fatal_error("out of memory in sarif_alloc_push_scope");
    }
    n->next = sarif_scope_overflow;
    sarif_scope_overflow = n;
    return &n->scope;
}

static void sarif_alloc_pop_scope(void) {
    if (sarif_scope_depth > 0) {
        sarif_scope_depth--;
        return;
    }
    struct SarifAllocScopeOverflow* n = sarif_scope_overflow;
    if (n != NULL) {
        sarif_scope_overflow = n->next;
        free(n);
    }
}

void sarif_alloc_push(void) {
    struct SarifAllocScope* scope = sarif_alloc_push_scope();
    if (scope == NULL) {
        return;
    }
    pthread_mutex_lock(&sarif_record_mutex);
    scope->chunk = sarif_record_current;
    scope->used = scope->chunk == NULL ? 0u : scope->chunk->used;
    pthread_mutex_unlock(&sarif_record_mutex);
}

void sarif_alloc_pop(void) {
    SarifRecordChunk* chunk = NULL;
    SarifRecordChunk* next = NULL;
    if (sarif_scope_depth == 0 && sarif_scope_overflow == NULL) {
        return;
    }
    struct SarifAllocScope scope = {
        .chunk = sarif_scope_depth > 0
            ? sarif_scope_stack[sarif_scope_depth - 1].chunk
            : sarif_scope_overflow->scope.chunk,
        .used = sarif_scope_depth > 0
            ? sarif_scope_stack[sarif_scope_depth - 1].used
            : sarif_scope_overflow->scope.used,
    };
    sarif_alloc_pop_scope();
    if (scope.chunk == NULL) {
        chunk = sarif_record_chunks;
        while (chunk != NULL) {
            next = chunk->next;
            free(chunk);
            chunk = next;
        }
        sarif_record_chunks = NULL;
        sarif_record_current = NULL;
        return;
    }
    chunk = scope.chunk->next;
    scope.chunk->next = NULL;
    while (chunk != NULL) {
        next = chunk->next;
        free(chunk);
        chunk = next;
    }
    sarif_record_current = scope.chunk;
    sarif_record_current->used = scope.used;
}

static inline __attribute__((always_inline)) __attribute__((unused)) void sarif_store_u64(unsigned char* base, uint64_t offset, uint64_t value) {
    memcpy(base + offset, &value, sizeof(uint64_t));
}

static inline __attribute__((always_inline)) __attribute__((unused)) uint64_t sarif_load_u64(const unsigned char* base, uint64_t offset) {
    uint64_t value;
    memcpy(&value, base + offset, sizeof(uint64_t));
    return value;
}

static int sarif_bytes_is_view(const unsigned char* bytes) {
    if (bytes == NULL) return 0;
    uint64_t tag = sarif_load_u64(bytes, 0);
    return (tag & SARIF_BYTES_VIEW_TAG) != 0;
}

static uint64_t sarif_bytes_view_len(const unsigned char* bytes) {
    return sarif_load_u64(bytes, 0) & ~SARIF_BYTES_VIEW_TAG;
}

static const unsigned char* sarif_bytes_view_data(const unsigned char* view) {
    uint64_t parent = sarif_load_u64(view, 8);
    uint64_t offset = sarif_load_u64(view, 16);
    return (const unsigned char*)(uintptr_t)parent + 8 + offset;
}

// Persistent string interning pool.
// Strings allocated here are never freed during program lifetime.
// Each unique content string is stored exactly once.
#define SARIF_INTERN_BUCKET_COUNT 262144u
#define SARIF_INTERN_CHUNK_SIZE (64u * 1024u)

struct SarifInternBucket {
    uint64_t hash;
    unsigned char* text;
};

struct SarifInternChunk {
    struct SarifInternChunk* next;
    size_t used;
    size_t cap;
    unsigned char data[];
};

static struct SarifInternBucket sarif_intern_table[SARIF_INTERN_BUCKET_COUNT];
static struct SarifInternChunk* sarif_intern_chunk = NULL;
static pthread_mutex_t sarif_intern_mutex = PTHREAD_MUTEX_INITIALIZER;

static uint64_t sarif_intern_hash(const unsigned char* data, uint64_t len) {
    uint64_t h = 14695981039346656037ULL;
    for (uint64_t i = 0; i < len; i++) {
        h ^= (uint64_t)data[i];
        h *= 1099511628211ULL;
    }
    return h;
}

// Caller must hold sarif_intern_mutex.
static unsigned char* sarif_intern_alloc(uint64_t size) {
    if (size > UINT64_MAX - 7u) {
        sarif_fatal_error("size overflow in string interning pool alignment");
    }
    uint64_t aligned_u64 = (size + 7u) & ~(uint64_t)7u;
    if (aligned_u64 < size) {
        sarif_fatal_error("size overflow in string interning pool alignment");
    }
    size_t aligned = (size_t)aligned_u64;
    if (sarif_intern_chunk == NULL || aligned > sarif_intern_chunk->cap - sarif_intern_chunk->used) {
        size_t chunk_size = sizeof(struct SarifInternChunk) + SARIF_INTERN_CHUNK_SIZE;
        if (chunk_size < sizeof(struct SarifInternChunk) + aligned) {
            chunk_size = sizeof(struct SarifInternChunk) + aligned;
        }
        struct SarifInternChunk* chunk = malloc(chunk_size);
        if (chunk == NULL) {
            sarif_fatal_error("out of memory in string interning pool");
        }
        chunk->next = sarif_intern_chunk;
        chunk->used = aligned;
        chunk->cap = chunk_size - sizeof(struct SarifInternChunk);
        sarif_intern_chunk = chunk;
        return chunk->data;
    }
    unsigned char* ptr = sarif_intern_chunk->data + sarif_intern_chunk->used;
    sarif_intern_chunk->used += aligned;
    return ptr;
}

static unsigned char* sarif_intern_find_or_insert(const unsigned char* data, uint64_t len) {
    pthread_mutex_lock(&sarif_intern_mutex);
    uint64_t hash = sarif_intern_hash(data, len);
    if (hash == 0) {
        hash = 1;
    }
    uint64_t idx = hash % SARIF_INTERN_BUCKET_COUNT;
    for (uint64_t probe = 0; probe < SARIF_INTERN_BUCKET_COUNT; probe++) {
        struct SarifInternBucket* b = &sarif_intern_table[idx];
        if (b->hash == 0) {
            unsigned char* interned = sarif_intern_alloc(8u + len);
            sarif_store_u64(interned, 0, len);
            if (len > 0) {
                memcpy(interned + 8, data, (size_t)len);
            }
            b->hash = hash;
            b->text = interned;
            pthread_mutex_unlock(&sarif_intern_mutex);
            return interned;
        }
        if (b->hash == hash) {
            uint64_t existing_len = sarif_load_u64(b->text, 0);
            if (existing_len == len && memcmp(b->text + 8, data, (size_t)len) == 0) {
                pthread_mutex_unlock(&sarif_intern_mutex);
                return b->text;
            }
        }
        idx = (idx + 1) % SARIF_INTERN_BUCKET_COUNT;
    }
    pthread_mutex_unlock(&sarif_intern_mutex);
    sarif_fatal_error("string interning table overflow");
}

// Intern a runtime text value into the persistent pool.
// If the same content was already interned, returns the existing pointer.
// The returned pointer is valid for the program's entire lifetime.
__attribute__((used)) const unsigned char* sarif_text_intern(const unsigned char* text) {
    if (text == NULL) return NULL;
    uint64_t len = sarif_load_u64(text, 0);
    return sarif_intern_find_or_insert(text + 8, len);
}

// Promote an arena-allocated text value to process lifetime.
// The returned pointer is valid for the program's entire lifetime
// and may be deduplicated with other promoted or interned text.
__attribute__((used)) const unsigned char* sarif_text_promote(const unsigned char* text) {
    return sarif_text_intern(text);
}

uint64_t sarif_text_len(const unsigned char* text) {
    if (text == NULL) { return 0; }
    return sarif_load_u64(text, 0);
}

static unsigned char* sarif_text_alloc(uint64_t len) {
    unsigned char* text = NULL;
    if (len > (uint64_t)SIZE_MAX - 8u) {
        return NULL;
    }
    text = (unsigned char*)sarif_record_alloc(8u + len);
    if (text == NULL) {
        return NULL;
    }
    sarif_store_u64(text, 0, len);
    return text;
}

static unsigned char* sarif_bytes_alloc(uint64_t len) {
    return sarif_text_alloc(len);
}

static unsigned char* sarif_text_alloc_extra(uint64_t len, uint64_t extra) {
    unsigned char* text = NULL;
    if (len > UINT64_MAX - extra || len + extra > (uint64_t)SIZE_MAX - 8u) {
        return NULL;
    }
    text = (unsigned char*)sarif_record_alloc(8u + len + extra);
    if (text == NULL) {
        return NULL;
    }
    sarif_store_u64(text, 0, len);
    return text;
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
    while (*end > 0 && *end < (int64_t)len && sarif_is_utf8_continuation(source[8 + *end])) {
        (*end)--;
    }
    if (*end < *start) {
        *end = *start;
    }
}

void* sarif_text_builder_new(void) {
    SarifTextBuilder* builder = (SarifTextBuilder*)malloc(sizeof(SarifTextBuilder));
    if (builder == NULL) {
        return NULL;
    }
    builder->len = 0;
    builder->cap = SARIF_TEXT_BUILDER_CHUNK;
    builder->bytes = (unsigned char*)malloc(SARIF_TEXT_BUILDER_CHUNK);
    if (builder->bytes == NULL) {
        free(builder);
        return NULL;
    }
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
            uint64_t growth = next_cap / 2u + 1u;
            if (UINT64_MAX - next_cap < growth) {
                next_cap = required;
                break;
            }
            next_cap += growth;
        }
    }
    if (next_cap > (uint64_t)SIZE_MAX) {
        return NULL;
    }
    grown = (unsigned char*)realloc(builder->bytes, (size_t)next_cap);
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
        encoded[0] = (unsigned char)(UTF8_CONT_PREFIX_1BYTE | (uint64_t)codepoint);
        encoded_len = 1;
    } else if (codepoint <= 0x7ff) {
        encoded[0] = (unsigned char)(UTF8_CONT_PREFIX_2BYTE | ((uint64_t)codepoint >> 6));
        encoded[1] = (unsigned char)(UTF8_CONTINUATION_MASK | ((uint64_t)codepoint & UTF8_DATA_MASK));
        encoded_len = 2;
    } else if (codepoint >= 0xd800 && codepoint <= 0xdfff) {
        return NULL;
    } else if (codepoint <= 0xffff) {
        encoded[0] = (unsigned char)(UTF8_CONT_PREFIX_3BYTE | ((uint64_t)codepoint >> 12));
        encoded[1] = (unsigned char)(UTF8_CONTINUATION_MASK | (((uint64_t)codepoint >> 6) & UTF8_DATA_MASK));
        encoded[2] = (unsigned char)(UTF8_CONTINUATION_MASK | ((uint64_t)codepoint & UTF8_DATA_MASK));
        encoded_len = 3;
    } else {
        encoded[0] = (unsigned char)(UTF8_CONT_PREFIX_4BYTE | ((uint64_t)codepoint >> 18));
        encoded[1] = (unsigned char)(UTF8_CONTINUATION_MASK | (((uint64_t)codepoint >> 12) & UTF8_DATA_MASK));
        encoded[2] = (unsigned char)(UTF8_CONTINUATION_MASK | (((uint64_t)codepoint >> 6) & UTF8_DATA_MASK));
        encoded[3] = (unsigned char)(UTF8_CONTINUATION_MASK | ((uint64_t)codepoint & UTF8_DATA_MASK));
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
    if (builder == NULL || byte < 0 || byte > 0xff) {
        return NULL;
    }
    if (builder->len + 1 > builder->cap) {
        builder = sarif_text_builder_reserve(builder, 1);
        if (builder == NULL) {
            return NULL;
        }
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
    int negative = (value < 0);
    if (negative) {
        magnitude = 0 - (uint64_t)value;
    } else {
        magnitude = (uint64_t)value;
    }
    do {
        scratch[--index] = (char)('0' + (magnitude % 10));
        magnitude /= 10;
    } while (magnitude != 0);
    if (negative) {
        scratch[--index] = '-';
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
    text = sarif_text_alloc(builder->len);
    if (text == NULL) {
        free(builder->bytes);
        free(builder);
        return NULL;
    }
    if (builder->len != 0) {
        memcpy(text + 8, builder->bytes, (size_t)builder->len);
    }
    free(builder->bytes);
    free(builder);
    return text;
}


static SarifList sarif_empty_list = { 0, NULL };

void* sarif_list_new(int64_t len, uint64_t fill) {
  SarifList* vec = NULL;
  uint64_t index = 0;
  if (len < 0 || (uint64_t)len > (uint64_t)SIZE_MAX / sizeof(uint64_t)) {
    return NULL;
  }
  if ((size_t)len == 0) {
    return &sarif_empty_list;
  }
  vec = malloc(sizeof(SarifList));
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

void* sarif_list_push(void* list_ptr, int64_t len, uint64_t value) {
    SarifList* list = (SarifList*)list_ptr;
    uint64_t used = 0;
    uint64_t next_cap = 0;
    uint64_t* grown = NULL;
    if (list == NULL || len < 0) { return NULL; }
    if (list->values == NULL && list->len == 0) {
        next_cap = 8u;
        grown = malloc((size_t)next_cap * sizeof(uint64_t));
        if (grown == NULL) { return NULL; }
        grown[0] = value;
        SarifList* vec = malloc(sizeof(SarifList));
        if (vec == NULL) { free(grown); return NULL; }
        vec->values = grown;
        vec->len = next_cap;
        return vec;
    }
    if (list->values == NULL && list->len > 0) { return NULL; }
    used = (uint64_t)len;
    if (used < list->len) {
        list->values[used] = value;
        return list;
    }
    if (used != list->len) {
        return NULL;
    }
    if (used == 0) {
        next_cap = 8u;
    } else if (used > UINT64_MAX / 2u) {
        next_cap = used + 1u;
    } else {
        next_cap = used * 2u;
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

uint64_t sarif_list_get(void* list_ptr, int64_t index) {
    SarifList* list = (SarifList*)list_ptr;
    if (list == NULL || list->values == NULL) {
        return 0;
    }
    if (index < 0 || (uint64_t)index >= list->len) {
        return 0;
    }
    return list->values[(uint64_t)index];
}

void* sarif_list_set(void* list_ptr, int64_t index, uint64_t value) {
    SarifList* list = (SarifList*)list_ptr;
    if (list == NULL || list->values == NULL) {
        return NULL;
    }
    if (index < 0 || (uint64_t)index >= list->len) {
        return NULL;
    }
    list->values[(uint64_t)index] = value;
    return list;
}

int64_t sarif_list_len(void* list_ptr) {
    SarifList* list = (SarifList*)list_ptr;
    if (list == NULL) {
        return 0;
    }
    return (int64_t)list->len;
}

// Build a list by copying len uint64_t slots from a packed record pointer.
// The record layout places fields consecutively at 8-byte offsets starting at ptr[0].
// Returns NULL on allocation failure or invalid arguments.
void* sarif_list_from_raw(void* raw_ptr, int64_t len) {
    SarifList* vec = NULL;
    if (raw_ptr == NULL || len <= 0 || (uint64_t)len > (uint64_t)SIZE_MAX / sizeof(uint64_t)) {
        if (len == 0) {
            return &sarif_empty_list;
        }
        return NULL;
    }
    vec = malloc(sizeof(SarifList));
    if (vec == NULL) {
        return NULL;
    }
    vec->len = (uint64_t)len;
    vec->values = malloc((size_t)len * sizeof(uint64_t));
    if (vec->values == NULL) {
        free(vec);
        return NULL;
    }
    memcpy(vec->values, raw_ptr, (size_t)len * sizeof(uint64_t));
    return vec;
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
static uint64_t sarif_sort_i32_field_offset = 0;
static uint64_t sarif_sort_f64_field_offset = 0;
static pthread_mutex_t sarif_sort_mutex = PTHREAD_MUTEX_INITIALIZER;

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

static int sarif_qsort_compare_record_i32_field_handles(const void* left, const void* right) {
    const uint64_t left_handle = *(const uint64_t*)left;
    const uint64_t right_handle = *(const uint64_t*)right;
    const unsigned char* left_record = (const unsigned char*)left_handle;
    const unsigned char* right_record = (const unsigned char*)right_handle;
    if (left_record == right_record) {
        return 0;
    }
    if (left_record == NULL) {
        return right_record == NULL ? 0 : -1;
    }
    if (right_record == NULL) {
        return 1;
    }
    int64_t left_val = (int64_t)sarif_load_u64(left_record, sarif_sort_i32_field_offset);
    int64_t right_val = (int64_t)sarif_load_u64(right_record, sarif_sort_i32_field_offset);
    if (left_val < right_val) return -1;
    if (left_val > right_val) return 1;
    return 0;
}

static int sarif_qsort_compare_record_f64_field_handles(const void* left, const void* right) {
    const uint64_t left_handle = *(const uint64_t*)left;
    const uint64_t right_handle = *(const uint64_t*)right;
    const unsigned char* left_record = (const unsigned char*)left_handle;
    const unsigned char* right_record = (const unsigned char*)right_handle;
    if (left_record == right_record) {
        return 0;
    }
    if (left_record == NULL) {
        return right_record == NULL ? 0 : -1;
    }
    if (right_record == NULL) {
        return 1;
    }
    double left_val;
    double right_val;
    memcpy(&left_val, left_record + sarif_sort_f64_field_offset, sizeof(double));
    memcpy(&right_val, right_record + sarif_sort_f64_field_offset, sizeof(double));
    if (left_val < right_val) return -1;
    if (left_val > right_val) return 1;
    return 0;
}

void* sarif_list_sort_text(void* list_ptr, int64_t len) {
    SarifList* list = (SarifList*)list_ptr;
    uint64_t used = 0;
    if (list == NULL || (list->values == NULL && list->len > 0) || len < 0) {
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
    if (list == NULL || (list->values == NULL && list->len > 0) || len < 0 || offset < 0) {
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

void* sarif_list_sort_by_i32_field(void* list_ptr, int64_t len, int64_t offset) {
    SarifList* list = (SarifList*)list_ptr;
    uint64_t used = 0;
    uint64_t field_offset = 0;
    if (list == NULL || (list->values == NULL && list->len > 0) || len < 0 || offset < 0) {
        return NULL;
    }
    used = (uint64_t)len;
    field_offset = (uint64_t)offset;
    if (used > list->len) {
        return NULL;
    }
    if (used > 1) {
        sarif_sort_i32_field_offset = field_offset;
        qsort(
            list->values,
            (size_t)used,
            sizeof(uint64_t),
            sarif_qsort_compare_record_i32_field_handles
        );
    }
    return list;
}

void* sarif_list_sort_by_f64_field(void* list_ptr, int64_t len, int64_t offset) {
    SarifList* list = (SarifList*)list_ptr;
    uint64_t used = 0;
    uint64_t field_offset = 0;
    if (list == NULL || (list->values == NULL && list->len > 0) || len < 0 || offset < 0) {
        return NULL;
    }
    used = (uint64_t)len;
    field_offset = (uint64_t)offset;
    if (used > list->len) {
        return NULL;
    }
    if (used > 1) {
        sarif_sort_f64_field_offset = field_offset;
        qsort(
            list->values,
            (size_t)used,
            sizeof(uint64_t),
            sarif_qsort_compare_record_f64_field_handles
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
    if (index == NULL || index->entries == NULL || index->cap == 0) {
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
    index->cap = 64;
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

int64_t sarif_text_index_contains(void* index_ptr, uint64_t key) {
    SarifTextIndex* index = (SarifTextIndex*)index_ptr;
    int found = 0;
    if (index == NULL || index->entries == NULL) {
        return 0;
    }
    sarif_text_index_find_entry(
        index,
        key,
        sarif_text_hash_handle(key),
        &found
    );
    return found;
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
void* sarif_text_index_keys(void* index_ptr) {
    SarifTextIndex* index = (SarifTextIndex*)index_ptr;
    if (index == NULL || index->entries == NULL) {
        return NULL;
    }
    void* builder = sarif_text_builder_new();
    if (builder == NULL) return NULL;
    for (uint64_t i = 0; i < index->cap; i++) {
        if (index->entries[i].occupied) {
            unsigned char* key_text = (unsigned char*)index->entries[i].key;
            builder = sarif_text_builder_append(builder, key_text);
            if (builder == NULL) { return NULL; }
            builder = sarif_text_builder_append_ascii(builder, (int64_t)'\n');
            if (builder == NULL) { return NULL; }
        }
    }
    return sarif_text_builder_finish(builder);
}

void* sarif_text_concat(const unsigned char* left, const unsigned char* right) {
    uint64_t left_len = 0;
    uint64_t right_len = 0;
    uint64_t total_len = 0;
    unsigned char* text = NULL;
    if (left == NULL || right == NULL) {
        return NULL;
    }
    left_len = sarif_load_u64(left, 0);
    right_len = sarif_load_u64(right, 0);
    if (left_len > UINT64_MAX - right_len) {
        return NULL;
    }
    total_len = left_len + right_len;
    if (total_len > (uint64_t)SIZE_MAX - 8u) {
        return NULL;
    }
    text = sarif_text_alloc(total_len);
    if (text == NULL) {
        return NULL;
    }
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
        while (cs < len && sarif_is_utf8_continuation(blob[8 + cs])) {
            cs++;
        }
        while (ce > 0 && ce < len && sarif_is_utf8_continuation(blob[8 + ce])) {
            ce--;
        }
        if (ce <= cs) return sarif_empty_text;
    } else {
        if (ce <= cs) return sarif_empty_text;
    }
    slen = (size_t)(ce - cs);
    result = sarif_text_alloc((uint64_t)slen);
    if (!result) return NULL;
    if (slen != 0) {
        memcpy(result + 8, blob + 8 + cs, slen);
    }
    return result;
}

void* sarif_text_slice(const unsigned char* text, uint64_t start, uint64_t end) {
    return sarif_slice_blob(text, start, end, 1);
}

void* sarif_bytes_slice(const unsigned char* bytes, uint64_t start, uint64_t end) {
    if (bytes == NULL) return NULL;
    uint64_t src_len = 0;
    uint64_t src_offset = 0;
    const unsigned char* src_parent = NULL;
    if (sarif_bytes_is_view(bytes)) {
        src_len = sarif_bytes_view_len(bytes);
        src_parent = (const unsigned char*)(uintptr_t)sarif_load_u64(bytes, 8);
        src_offset = sarif_load_u64(bytes, 16);
    } else {
        src_len = sarif_load_u64(bytes, 0);
        src_parent = bytes;
        src_offset = 0;
    }
    uint64_t cs = start < src_len ? start : src_len;
    uint64_t ce = end < src_len ? end : src_len;
    if (ce <= cs) return sarif_empty_text;
    uint64_t view_len = ce - cs;
    unsigned char* view = (unsigned char*)sarif_record_alloc(24);
    if (view == NULL) return NULL;
    sarif_store_u64(view, 0, view_len | SARIF_BYTES_VIEW_TAG);
    sarif_store_u64(view, 8, (uint64_t)(uintptr_t)src_parent);
    sarif_store_u64(view, 16, src_offset + cs);
    return view;
}

void* sarif_text_from_f64_fixed(double value, int64_t digits) {
    int precision = 0;
    int len = 0;
    unsigned char* result = NULL;
    if (digits > 0) {
        precision = digits > 1000 ? 1000 : (int)digits;
    }
    // Fast path for integer values - avoid snprintf overhead
    if (sarif_should_use_integer_fastpath(value, precision)) {
        int64_t int_part = (int64_t)value;
        char scratch[32];
        int idx = 32;
        uint64_t mag;
        int negative = 0;
        if (int_part < 0) {
            negative = 1;
            mag = (uint64_t)(-int_part);
        } else {
            mag = (uint64_t)int_part;
        }
        if (mag == 0) {
            scratch[--idx] = '0';
        } else {
            while (mag > 0) {
                scratch[--idx] = '0' + (char)(mag % 10);
                mag /= 10;
            }
        }
        if (negative) {
            scratch[--idx] = '-';
        }
        len = 32 - idx;
        result = sarif_text_alloc_extra((uint64_t)len, 1u);
        if (result == NULL) {
            return NULL;
        }
        memcpy(result + 8, scratch + idx, (size_t)len);
        return result;
    }
    len = snprintf(NULL, 0, "%.*f", precision, value);
    if (len < 0) {
        return NULL;
    }
    result = sarif_text_alloc_extra((uint64_t)len, 1u);
    if (result == NULL) {
        return NULL;
    }
    if (len != 0) {
        snprintf((char*)(result + 8), (size_t)len + 1u, "%.*f", precision, value);
    }
    return result;
}

static int sarif_parse_i32_core_checked(const unsigned char* bytes, uint64_t index, uint64_t len, int32_t* out_value) {
    uint64_t limit;
    int negative = 0;
    int64_t value = 0;
    if (out_value == NULL) return 0;
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
    *out_value = (int32_t)(negative ? -value : value);
    return 1;
}

static int64_t sarif_parse_i32_core(const unsigned char* bytes, uint64_t index, uint64_t len) {
    int64_t value = 0;
    if (!sarif_parse_i32_core_checked(bytes, index, len, &value)) return 0;
    return value;
}

int64_t sarif_parse_i32(const unsigned char* text) {
    uint64_t len;
    if (text == NULL) return 0;
    len = sarif_load_u64(text, 0);
    if (len == 0) return 0;
    return sarif_parse_i32_core(text + 8, 0, len);
}

int64_t sarif_parse_i32_range(const unsigned char* text, int64_t start, int64_t end) {
    uint64_t len, index;
    const unsigned char* bytes;
    if (text == NULL) return 0;
    len = sarif_load_u64(text, 0);
    index = start > 0 ? (uint64_t)start < len ? (uint64_t)start : len : 0;
    len = end > 0 ? (uint64_t)end < len ? (uint64_t)end : len : 0;
    bytes = text + 8;
    while (index < len && bytes[index] == ' ') {
        index += 1;
    }
    while (len > index && bytes[len - 1] == ' ') {
        len -= 1;
    }
    return sarif_parse_i32_core(bytes, index, len);
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
    if (index >= 0 && sarif_argv != NULL && index < sarif_argc) {
        value = sarif_argv[index];
    }
    len = strlen(value);
    unsigned char* result = (unsigned char*)malloc(8u + len);
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

    while (1) {
        if (len > SIZE_MAX - sizeof(chunk)) {
            free(buffer);
            return NULL;
        }
        read = fread(chunk, 1u, sizeof(chunk), stdin);
        if (read == 0u) {
            break;
        }
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

static int __attribute__((unused)) sarif_write_i64(int64_t value, int newline) {
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

uint64_t sarif_file_open(const unsigned char* path_handle, const unsigned char* mode_handle) {
    if (path_handle == NULL || mode_handle == NULL) {
        return 0;
    }
    uint64_t path_len = sarif_load_u64(path_handle, 0);
    uint64_t mode_len = sarif_load_u64(mode_handle, 0);
    if (path_len == 0 || mode_len == 0 || path_len > (uint64_t)(SIZE_MAX - 1) || mode_len > (uint64_t)(SIZE_MAX - 1)) {
        return 0;
    }
    char* path = (char*)malloc((size_t)path_len + 1);
    char* mode = (char*)malloc((size_t)mode_len + 1);
    if (path == NULL || mode == NULL) {
        free(path);
        free(mode);
        return 0;
    }
    memcpy(path, path_handle + 8, (size_t)path_len);
    path[path_len] = '\0';
    memcpy(mode, mode_handle + 8, (size_t)mode_len);
    mode[mode_len] = '\0';
    FILE* f = fopen(path, mode);
    free(path);
    free(mode);
    return (uint64_t)(uintptr_t)f;
}

void sarif_file_close(uint64_t handle) {
    FILE* f = (FILE*)(uintptr_t)handle;
    if (f != NULL) {
        fclose(f);
    }
}

void sarif_file_sync(uint64_t handle) {
    FILE* f = (FILE*)(uintptr_t)handle;
    if (f != NULL) {
        if (fflush(f) != 0) {
            sarif_fatal_error("fflush failed in sarif_file_sync");
        }
        if (fsync(fileno(f)) != 0) {
            sarif_fatal_error("fsync failed in sarif_file_sync");
        }
    }
}

uint64_t sarif_file_read(uint64_t handle, int64_t len) {
    FILE* f = (FILE*)(uintptr_t)handle;
    if (f == NULL || len < 0) {
        return (uint64_t)NULL;
    }
    unsigned char* bytes = sarif_bytes_alloc((uint64_t)len);
    if (bytes == NULL) {
        return (uint64_t)NULL;
    }
    size_t n = fread(bytes + 8, 1, (size_t)len, f);
    sarif_store_u64(bytes, 0, (uint64_t)n);
    return (uint64_t)(uintptr_t)bytes;
}

uint64_t sarif_file_read_to_end(uint64_t handle) {
    FILE* f = (FILE*)(uintptr_t)handle;
    if (f == NULL) {
        return (uint64_t)NULL;
    }
    long current = ftell(f);
    if (current < 0) {
        return (uint64_t)NULL;
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        return (uint64_t)NULL;
    }
    long end = ftell(f);
    if (end < 0) {
        return (uint64_t)NULL;
    }
    if (fseek(f, current, SEEK_SET) != 0) {
        return (uint64_t)NULL;
    }
    long len = end - current;
    if (len < 0) {
        len = 0;
    }
    unsigned char* bytes = sarif_bytes_alloc((uint64_t)len);
    if (bytes == NULL) {
        return (uint64_t)NULL;
    }
    if (len > 0) {
        size_t n = fread(bytes + 8, 1, (size_t)len, f);
        sarif_store_u64(bytes, 0, (uint64_t)n);
    } else {
        sarif_store_u64(bytes, 0, 0);
    }
    return (uint64_t)(uintptr_t)bytes;
}


int64_t sarif_file_write(uint64_t handle, const unsigned char* data_handle) {
    FILE* f = (FILE*)(uintptr_t)handle;
    if (f == NULL || data_handle == NULL) {
        return -1;
    }
    uint64_t len = sarif_load_u64(data_handle, 0);
    if (len == 0) {
        return 0;
    }
    size_t n = fwrite(data_handle + 8, 1, (size_t)len, f);
    return (int64_t)n;
}

int64_t sarif_file_seek(uint64_t handle, int64_t offset, int64_t whence) {
    FILE* f = (FILE*)(uintptr_t)handle;
    if (f == NULL) {
        return -1;
    }
    int w = SEEK_SET;
    if (whence == 1) w = SEEK_CUR;
    else if (whence == 2) w = SEEK_END;
    if (fseek(f, (long)offset, w) != 0) {
        return -1;
    }
    return (int64_t)ftell(f);
}

int64_t sarif_file_size(uint64_t handle) {
    FILE* f = (FILE*)(uintptr_t)handle;
    if (f == NULL) {
        return -1;
    }
    long current = ftell(f);
    if (current < 0) {
        return -1;
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        return -1;
    }
    long size = ftell(f);
    if (size < 0) {
        (void)fseek(f, current, SEEK_SET);
        return -1;
    }
    if (fseek(f, current, SEEK_SET) != 0) {
        return -1;
    }
    return (int64_t)size;
}

int64_t sarif_file_exists(const unsigned char* path_handle) {
    if (path_handle == NULL) return 0;
    uint64_t len = sarif_load_u64(path_handle, 0);
    if (len > (uint64_t)(SIZE_MAX - 1)) return 0;
    char* path = (char*)malloc((size_t)len + 1);
    if (path == NULL) return 0;
    memcpy(path, path_handle + 8, (size_t)len);
    path[len] = '\0';
    int64_t result = access(path, F_OK) == 0 ? 1 : 0;
    free(path);
    return result;
}

int64_t sarif_file_remove(const unsigned char* path_handle) {
    if (path_handle == NULL) return 0;
    uint64_t len = sarif_load_u64(path_handle, 0);
    if (len > (uint64_t)(SIZE_MAX - 1)) return 0;
    char* path = (char*)malloc((size_t)len + 1);
    if (path == NULL) return 0;
    memcpy(path, path_handle + 8, (size_t)len);
    path[len] = '\0';
    int64_t result = remove(path) == 0 ? 1 : 0;
    free(path);
    return result;
}

int64_t sarif_file_is_valid(uint64_t handle) {
    return handle != 0 ? 1 : 0;
}

#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <signal.h>

#ifndef MSG_NOSIGNAL
#define MSG_NOSIGNAL 0
#endif

#if !defined(MSG_NOSIGNAL) || (MSG_NOSIGNAL == 0)
static pthread_once_t sarif_sigpipe_once = PTHREAD_ONCE_INIT;

static void sarif_ignore_sigpipe_once(void) {
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = SIG_IGN;
    (void)sigemptyset(&sa.sa_mask);
    (void)sigaction(SIGPIPE, &sa, NULL);
}

static void sarif_init_sigpipe_handling_if_needed(void) {
    (void)pthread_once(&sarif_sigpipe_once, sarif_ignore_sigpipe_once);
}
#else
static void sarif_init_sigpipe_handling_if_needed(void) {
}
#endif

uint64_t sarif_file_mmap(const unsigned char* path_handle) {
    sarif_init_sigpipe_handling_if_needed();
    if (path_handle == NULL) {
        return (uint64_t)NULL;
    }
    uint64_t path_len = sarif_load_u64(path_handle, 0);
    if (path_len > (uint64_t)(SIZE_MAX - 1)) {
        return (uint64_t)NULL;
    }
    size_t path_size = (size_t)path_len;
    char* path = (char*)malloc(path_size + 1);
    if (path == NULL) {
        return (uint64_t)NULL;
    }
    memcpy(path, path_handle + 8, path_size);
    path[path_size] = '\0';

    int fd = open(path, O_RDONLY);
    free(path);
    if (fd < 0) {
        return (uint64_t)NULL;
    }
    struct stat st;
    if (fstat(fd, &st) < 0) {
        close(fd);
        return (uint64_t)NULL;
    }
    size_t size = st.st_size;
    if (size < 8) {
        close(fd);
        return (uint64_t)NULL;
    }
    void* addr = mmap(NULL, size, PROT_READ, MAP_SHARED, fd, 0);
    close(fd);
    if (addr == MAP_FAILED) {
        return (uint64_t)NULL;
    }
    uint64_t file_data_len = 0;
    memcpy(&file_data_len, addr, sizeof(file_data_len));
    if (file_data_len + 8 > size) {
        munmap(addr, size);
        return (uint64_t)NULL;
    }
    return (uint64_t)(uintptr_t)addr;
}

uint64_t sarif_tcp_listen(int64_t port) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return 0;
    int one = 1;
    if (setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one)) < 0) {
        close(fd);
        return 0;
    }
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = htons((uint16_t)port);
    if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        close(fd);
        return 0;
    }
    if (listen(fd, 8) < 0) {
        close(fd);
        return 0;
    }
    return (uint64_t)fd;
}

uint64_t sarif_tcp_accept(uint64_t server_fd) {
    struct sockaddr_in cli;
    socklen_t len = sizeof(cli);
    int client = accept((int)server_fd, (struct sockaddr*)&cli, &len);
    if (client < 0) return 0;
    return (uint64_t)client;
}

uint64_t sarif_tcp_recv(uint64_t fd, int64_t max_len) {
    if (fd == 0 || max_len <= 0) return (uint64_t)NULL;
    unsigned char* bytes = sarif_bytes_alloc((uint64_t)max_len);
    if (bytes == NULL) return (uint64_t)NULL;
    ssize_t n = recv((int)fd, bytes + 8, (size_t)max_len, 0);
    if (n < 0) return (uint64_t)NULL;
    sarif_store_u64(bytes, 0, (uint64_t)n);
    return (uint64_t)(uintptr_t)bytes;
}

int64_t sarif_tcp_send(uint64_t fd, const unsigned char* data_handle) {
    if (fd == 0 || data_handle == NULL) return -1;
    uint64_t len = sarif_load_u64(data_handle, 0);
    if (len == 0) return 0;
    ssize_t n = send((int)fd, data_handle + 8, (size_t)len, MSG_NOSIGNAL);
    if (n < 0) return -1;
    return (int64_t)n;
}

void sarif_tcp_close(uint64_t fd) {
    if (fd != 0) close((int)fd);
}

uint64_t sarif_bytes_len(const unsigned char* bytes) {
    if (bytes == NULL) return 0;
    if (sarif_bytes_is_view(bytes)) {
        return sarif_bytes_view_len(bytes);
    }
    return sarif_load_u64(bytes, 0);
}

int64_t sarif_bytes_byte(const unsigned char* bytes, uint64_t index) {
    if (bytes == NULL) return 0;
    if (sarif_bytes_is_view(bytes)) {
        uint64_t len = sarif_bytes_view_len(bytes);
        if (index >= len) return 0;
        const unsigned char* data = sarif_bytes_view_data(bytes);
        return (int64_t)data[index];
    }
    uint64_t len = sarif_load_u64(bytes, 0);
    if (index >= len) return 0;
    return (int64_t)bytes[8 + index];
}

void* sarif_bytes_materialize(const unsigned char* bytes) {
    if (bytes == NULL) return NULL;
    if (!sarif_bytes_is_view(bytes)) {
        uint64_t len = sarif_load_u64(bytes, 0);
        unsigned char* result = sarif_bytes_alloc(len);
        if (result == NULL) return NULL;
        if (len != 0) memcpy(result + 8, bytes + 8, (size_t)len);
        return result;
    }
    uint64_t len = sarif_bytes_view_len(bytes);
    const unsigned char* data = sarif_bytes_view_data(bytes);
    unsigned char* result = sarif_bytes_alloc(len);
    if (result == NULL) return NULL;
    if (len != 0) memcpy(result + 8, data, (size_t)len);
    return result;
}

void* sarif_bytes_to_text(const unsigned char* bytes) {
    if (bytes == NULL) return NULL;
    if (sarif_bytes_is_view(bytes)) {
        uint64_t len = sarif_bytes_view_len(bytes);
        const unsigned char* data = sarif_bytes_view_data(bytes);
        unsigned char* result = sarif_text_alloc(len);
        if (result == NULL) return NULL;
        if (len != 0) memcpy(result + 8, data, (size_t)len);
        return result;
    }
    uint64_t len = sarif_load_u64(bytes, 0);
    unsigned char* result = sarif_text_alloc(len);
    if (result == NULL) return NULL;
    if (len != 0) memcpy(result + 8, bytes + 8, (size_t)len);
    return result;
}

unsigned char* sarif_text_to_bytes(const char* text) {
    const unsigned char* bytes = (const unsigned char*)text;
    if (bytes == NULL) return NULL;
    if (sarif_bytes_is_view(bytes)) {
        uint64_t len = sarif_bytes_view_len(bytes);
        const unsigned char* data = sarif_bytes_view_data(bytes);
        unsigned char* result = sarif_bytes_alloc(len);
        if (result == NULL) return NULL;
        if (len != 0) memcpy(result + 8, data, (size_t)len);
        return result;
    }
    uint64_t len = sarif_load_u64(bytes, 0);
    unsigned char* result = sarif_bytes_alloc(len);
    if (result == NULL) return NULL;
    if (len != 0) memcpy(result + 8, bytes + 8, (size_t)len);
    return result;
}

static int sarif_str_eq(const char* a, const char* b) {
    while (*a && *b) {
        if (*a != *b) return 0;
        a++; b++;
    }
    return *a == *b;
}

typedef int64_t (*sarif_effect_handler_t)(uint64_t* args, int32_t nargs);

struct SarifEffectHandler {
    const char* effect;
    const char* operation;
    sarif_effect_handler_t handler;
};

static const struct SarifEffectHandler* sarif_find_handler(
    const char* effect, const char* operation,
    sarif_effect_handler_t* out_handler
) {
    extern const struct SarifEffectHandler sarif_effect_table[];
    extern const size_t sarif_effect_table_len;
    for (size_t i = 0; i < sarif_effect_table_len; i++) {
        if (sarif_str_eq(sarif_effect_table[i].effect, effect) &&
            sarif_str_eq(sarif_effect_table[i].operation, operation)) {
            *out_handler = sarif_effect_table[i].handler;
            return &sarif_effect_table[i];
        }
    }
    return NULL;
}

int64_t sarif_perform_effect(const char* effect, const char* operation,
    uint64_t arg0, uint64_t arg1,
    uint64_t arg2, uint64_t arg3, int32_t nargs) {
    sarif_effect_handler_t handler = NULL;
    if (sarif_find_handler(effect, operation, &handler)) {
        uint64_t args[8];
        args[0] = arg0; args[1] = arg1; args[2] = arg2; args[3] = arg3;
        return handler(args, nargs);
    }
    return 0;
}

int64_t sarif_env_get(const unsigned char* key_handle) {
    uint64_t key_len = sarif_load_u64(key_handle, 0);
    const unsigned char* key_data = key_handle + 8;
    char* key = (char*)malloc(key_len + 1);
    if (key == NULL) {
        return (int64_t)sarif_empty_text;
    }
    memcpy(key, key_data, key_len);
    key[key_len] = '\0';
    pthread_mutex_lock(&sarif_env_mutex);
    const char* val = getenv(key);
    if (val == NULL) {
        pthread_mutex_unlock(&sarif_env_mutex);
        free(key);
        return (int64_t)sarif_empty_text;
    }
    uint64_t val_len = strlen(val);
    unsigned char* result = sarif_text_alloc(val_len);
    if (result == NULL) {
        pthread_mutex_unlock(&sarif_env_mutex);
        free(key);
        return (int64_t)sarif_empty_text;
    }
    memcpy(result + 8, val, val_len);
    pthread_mutex_unlock(&sarif_env_mutex);
    free(key);
    return (int64_t)result;
}

int64_t sarif_env_set(const unsigned char* key_handle, const unsigned char* value_handle) {
    uint64_t key_len = sarif_load_u64(key_handle, 0);
    const unsigned char* key_data = key_handle + 8;
    uint64_t val_len = sarif_load_u64(value_handle, 0);
    const unsigned char* val_data = value_handle + 8;
    char* key = (char*)malloc(key_len + 1);
    if (key == NULL) {
        return 0;
    }
    memcpy(key, key_data, key_len);
    key[key_len] = '\0';
    char* val = (char*)malloc(val_len + 1);
    if (val == NULL) {
        free(key);
        return 0;
    }
    memcpy(val, val_data, val_len);
    val[val_len] = '\0';
    pthread_mutex_lock(&sarif_env_mutex);
    int rc = setenv(key, val, 1);
    pthread_mutex_unlock(&sarif_env_mutex);
    free(key);
    free(val);
    return rc == 0 ? 1 : 0;
}

int64_t sarif_env_remove(const unsigned char* key_handle) {
    uint64_t key_len = sarif_load_u64(key_handle, 0);
    const unsigned char* key_data = key_handle + 8;
    char* key = (char*)malloc(key_len + 1);
    if (key == NULL) {
        return 0;
    }
    memcpy(key, key_data, key_len);
    key[key_len] = '\0';
    pthread_mutex_lock(&sarif_env_mutex);
    int rc = unsetenv(key);
    pthread_mutex_unlock(&sarif_env_mutex);
    free(key);
    return rc == 0 ? 1 : 0;
}

int64_t sarif_env_keys(void) {
#if defined(__APPLE__)
    extern char*** _NSGetEnviron(void);
    char** envp = *_NSGetEnviron();
#else
    extern char** environ;
    char** envp = environ;
#endif
    uint64_t total_len = 0;
    pthread_mutex_lock(&sarif_env_mutex);
    for (int i = 0; envp[i] != NULL; i++) {
        char* eq = strchr(envp[i], '=');
        total_len += (eq ? (uint64_t)(eq - envp[i]) : strlen(envp[i]));
        if (i > 0) total_len += 1;
    }
    unsigned char* result = sarif_text_alloc(total_len);
    if (result == NULL) {
        pthread_mutex_unlock(&sarif_env_mutex);
        return (int64_t)sarif_empty_text;
    }
    uint64_t offset = 0;
    for (int i = 0; envp[i] != NULL; i++) {
        if (i > 0) {
            result[8 + offset] = '\n';
            offset += 1;
        }
        char* eq = strchr(envp[i], '=');
        uint64_t name_len = eq ? (uint64_t)(eq - envp[i]) : strlen(envp[i]);
        memcpy(result + 8 + offset, envp[i], name_len);
        offset += name_len;
    }
    pthread_mutex_unlock(&sarif_env_mutex);
    return (int64_t)result;
}

int64_t sarif_dir_create(const unsigned char* path_handle) {
    uint64_t path_len = sarif_load_u64(path_handle, 0);
    const unsigned char* path_data = path_handle + 8;
    char* path = (char*)malloc(path_len + 1);
    if (path == NULL) {
        return 0;
    }
    memcpy(path, path_data, path_len);
    path[path_len] = '\0';
    int result = mkdir(path, 0755);
    free(path);
    if (result == 0) return 1;
    if (errno == EEXIST) return 1;
    return 0;
}

int64_t sarif_dir_remove(const unsigned char* path_handle) {
    uint64_t path_len = sarif_load_u64(path_handle, 0);
    const unsigned char* path_data = path_handle + 8;
    char* path = (char*)malloc(path_len + 1);
    if (path == NULL) {
        return 0;
    }
    memcpy(path, path_data, path_len);
    path[path_len] = '\0';
    int result = rmdir(path);
    free(path);
    if (result == 0) return 1;
    return (errno == ENOENT) ? 1 : 0;
}

int64_t sarif_dir_list(const unsigned char* path_handle) {
    uint64_t path_len = sarif_load_u64(path_handle, 0);
    const unsigned char* path_data = path_handle + 8;
    char* path = (char*)malloc(path_len + 1);
    if (path == NULL) {
        return (int64_t)sarif_empty_text;
    }
    memcpy(path, path_data, path_len);
    path[path_len] = '\0';
    DIR* dir = opendir(path);
    free(path);
    if (dir == NULL) return (int64_t)sarif_empty_text;
    uint64_t total_len = 0;
    struct dirent* entry;
    int count = 0;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        total_len += strlen(entry->d_name);
        if (count > 0) total_len += 1;
        count++;
    }
    rewinddir(dir);
    unsigned char* result = sarif_text_alloc(total_len);
    uint64_t offset = 0;
    count = 0;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        if (count > 0) {
            result[8 + offset] = '\n';
            offset += 1;
        }
        uint64_t name_len = strlen(entry->d_name);
        memcpy(result + 8 + offset, entry->d_name, name_len);
        offset += name_len;
        count++;
    }
    closedir(dir);
    return (int64_t)result;
}

int64_t sarif_dir_exists(const unsigned char* path_handle) {
    uint64_t path_len = sarif_load_u64(path_handle, 0);
    const unsigned char* path_data = path_handle + 8;
    char* path = (char*)malloc(path_len + 1);
    if (path == NULL) {
        return 0;
    }
    memcpy(path, path_data, path_len);
    path[path_len] = '\0';
    struct stat st;
    int result = stat(path, &st) == 0 && S_ISDIR(st.st_mode);
    free(path);
    return result ? 1 : 0;
}

int64_t sarif_dir_current(void) {
    char* cwd = getcwd(NULL, 0);
    if (cwd == NULL) return (int64_t)sarif_empty_text;
    uint64_t len = strlen(cwd);
    unsigned char* result = sarif_text_alloc(len);
    if (result == NULL) {
        free(cwd);
        return (int64_t)sarif_empty_text;
    }
    memcpy(result + 8, cwd, len);
    free(cwd);
    return (int64_t)result;
}

int64_t sarif_dir_change(const unsigned char* path_handle) {
    uint64_t path_len = sarif_load_u64(path_handle, 0);
    const unsigned char* path_data = path_handle + 8;
    char* path = (char*)malloc(path_len + 1);
    if (path == NULL) {
        return 0;
    }
    memcpy(path, path_data, path_len);
    path[path_len] = '\0';
    int result = chdir(path);
    free(path);
    return result == 0 ? 1 : 0;
}

void sarif_process_exit(int64_t code) {
    exit((int)code);
}

int64_t sarif_process_id(void) {
    return (int64_t)getpid();
}

double sarif_clock_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}

void sarif_clock_sleep(int64_t ms) {
    struct timespec req;
    req.tv_sec = (time_t)(ms / 1000);
    req.tv_nsec = (long)((ms % 1000) * 1000000);
    nanosleep(&req, NULL);
}

static int sarif_runtime_check(void) {
  uint64_t i = 0;
  struct SarifRecordChunk* chunk = NULL;
  struct SarifInternBucket* bucket = NULL;
  (void)i;
  for (i = 0; i < SARIF_SCOPE_STACK_CAP; i++) {
    if (sarif_scope_stack[i].chunk != NULL) {
      chunk = sarif_scope_stack[i].chunk;
      while (chunk != NULL) {
        if (chunk->used > chunk->cap) {
          return -1;
        }
        chunk = chunk->next;
      }
    }
  }
  if (sarif_scope_depth >= SARIF_SCOPE_STACK_CAP && sarif_scope_overflow != NULL) {
    chunk = sarif_scope_overflow->scope.chunk;
    while (chunk != NULL) {
      if (chunk->used > chunk->cap) {
        return -2;
      }
      chunk = chunk->next;
    }
  }
  for (i = 0; i < SARIF_INTERN_BUCKET_COUNT; i++) {
    bucket = &sarif_intern_table[i];
    if (bucket->hash != 0) {
      if (bucket->text == NULL) {
        return -3;
      }
      uint64_t interned_len = 0;
      memcpy(&interned_len, bucket->text, sizeof(uint64_t));
      if (interned_len > SIZE_MAX - 8) {
        return -5;
      }
    }
  }
  if (sarif_record_current != NULL && sarif_record_current->used > sarif_record_current->cap) {
    return -4;
  }
  return 0;
}

int main(int argc, char** argv) {
  sarif_argc = argc;
  sarif_argv = argv;
  /* Intentionally use full buffering for stdout; with NULL and size 0, the C
     runtime allocates a buffer and chooses an implementation-appropriate size. */
  setvbuf(stdout, NULL, _IOFBF, 0);
  if (sarif_runtime_check() != 0) {
    fprintf(stderr, "SARIF RUNTIME CHECK FAILED\n");
    return 1;
  }
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
    if (sarif_write_text_blob(text, 0) != 0) {
        return 1;
    }
    return sarif_write_byte('\n') != 0 ? 1 : 0;
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
