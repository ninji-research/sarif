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

typedef struct SarifRecordDesc SarifRecordDesc;
typedef struct SarifEnumDesc SarifEnumDesc;
typedef struct SarifVariantDesc SarifVariantDesc;
typedef struct SarifTextBuilder SarifTextBuilder;
typedef struct SarifF64Vec SarifF64Vec;

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

struct SarifF64Vec {
    uint64_t len;
    double* values;
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

void* sarif_record_alloc(uint64_t size) {
    return calloc((size_t)size, 1);
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

void* sarif_text_builder_append(void* raw_builder, const unsigned char* text) {
    SarifTextBuilder* builder = (SarifTextBuilder*)raw_builder;
    uint64_t text_len = 0;
    uint64_t required = 0;
    uint64_t next_cap = 0;
    unsigned char* grown = NULL;
    if (builder == NULL || text == NULL) {
        return NULL;
    }
    text_len = sarif_load_u64(text, 0);
    if (text_len == 0) {
        return builder;
    }
    if (builder->len > UINT64_MAX - text_len) {
        return NULL;
    }
    required = builder->len + text_len;
    if (required > builder->cap) {
        next_cap = builder->cap == 0 ? 64u : builder->cap;
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
    }
    memcpy(builder->bytes + builder->len, text + 8, (size_t)text_len);
    builder->len = required;
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

void* sarif_f64_vec_new(int64_t len, uint64_t fill_bits) {
    SarifF64Vec* vec = NULL;
    double fill = 0.0;
    uint64_t index = 0;
    if (len < 0) {
        return NULL;
    }
    memcpy(&fill, &fill_bits, sizeof(fill));
    if ((uint64_t)len > (uint64_t)SIZE_MAX / sizeof(double)) {
        return NULL;
    }
    vec = calloc(1u, sizeof(SarifF64Vec));
    if (vec == NULL) {
        return NULL;
    }
    vec->len = (uint64_t)len;
    if (fill_bits == 0) {
        vec->values = calloc((size_t)len, sizeof(double));
        if (vec->values == NULL) {
            free(vec);
            return NULL;
        }
    } else {
        vec->values = malloc((size_t)len * sizeof(double));
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

void* sarif_text_from_f64_fixed(uint64_t bits, int64_t digits) {
    double value = 0.0;
    int precision = 0;
    int len = 0;
    unsigned char* result = NULL;
    memcpy(&value, &bits, sizeof(value));
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
    char* buffer = NULL;
    char* end = NULL;
    long long value = 0;
    if (text == NULL) {
        return 0;
    }
    len = sarif_load_u64(text, 0);
    if (len > (uint64_t)SIZE_MAX - 1u) {
        return 0;
    }
    buffer = malloc((size_t)len + 1u);
    if (buffer == NULL) {
        return 0;
    }
    if (len != 0) {
        memcpy(buffer, text + 8, (size_t)len);
    }
    buffer[len] = '\0';
    errno = 0;
    value = strtoll(buffer, &end, 10);
    if (end == buffer || *end != '\0' || errno != 0 || value < INT32_MIN || value > INT32_MAX) {
        free(buffer);
        return 0;
    }
    free(buffer);
    return (int64_t)value;
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
    int byte = 0;

    if (sarif_stdin_cache != NULL) {
        return sarif_stdin_cache;
    }

    while ((byte = fgetc(stdin)) != EOF) {
        if (len == cap) {
            size_t next_cap = cap == 0 ? 4096u : cap * 2u;
            unsigned char* next = realloc(buffer, next_cap);
            if (next == NULL) {
                free(buffer);
                return NULL;
            }
            buffer = next;
            cap = next_cap;
        }
        buffer[len++] = (unsigned char)byte;
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
    return sarif_write_text_blob(text, 1);
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
