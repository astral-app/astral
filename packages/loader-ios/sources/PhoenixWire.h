#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint8_t *bytes;
    size_t length;
} AstralWireBuffer;

typedef enum {
    kAstralRowLink,
    kAstralRowSwitch,
    kAstralRowInfo,
    kAstralRowSwitchLink,
    kAstralRowSelected,
    kAstralRowChoice,
} AstralRowKind;

typedef struct {
    const char *key;
    const char *title;
    const char *subtitle;
    const char *detail;
    const char *icon;
    AstralRowKind kind;
    int on;
} AstralRowSpec;

typedef struct {
    const char *title;
    const AstralRowSpec *rows;
    size_t count;
} AstralSectionSpec;

typedef struct {
    const char *key;
    const char *title;
    int selected;
} AstralChipSpec;

typedef struct {
    const AstralChipSpec *chips;
    size_t chipCount;
    const char *buttonKey;
    const char *buttonIcon;
} AstralToolbarSpec;

typedef struct {
    const char *key;
    const char *title;
    const char *authors;
    const char *description;
    const char *version;
    const char *icon;
    const char *error;
    const char *avatars[3];
    size_t avatarCount;
    int hasSettings;
    int on;
    int failed;
} AstralCardSpec;

typedef struct {
    const char *title;
    const AstralSectionSpec *sections;
    size_t count;
    const AstralToolbarSpec *toolbar;
    const AstralCardSpec *cards;
    size_t cardCount;
} AstralPageSpec;

typedef struct {
    const char *prefix;
    uint64_t message_id;
    uint64_t user_id;
    uint64_t channel_id;
} AstralDecorationContext;

typedef enum {
    kAstralTagPill,
    kAstralTagOriginalPoster,
} AstralTagShape;

typedef struct {
    char text[32];
    uint32_t background;
    uint32_t foreground;
    uint32_t gradient[3];
    uint32_t gradientCount;
    AstralTagShape shape;
} AstralTag;

typedef size_t (*AstralDecorator)(const AstralDecorationContext *context, AstralTag *tags, size_t capacity);

typedef enum {
    kAstralLabelAuthor,
    kAstralLabelReply,
} AstralLabelKind;

typedef struct {
    AstralLabelKind kind;
    uint64_t message_id;
    uint64_t channel_id;
} AstralLabelContext;

typedef int (*AstralRelabeler)(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity);

typedef struct {
    uint64_t authors;
    uint64_t with_user;
    uint64_t with_channel;
    uint64_t tagged;
} AstralWireStats;

void astral_wire_set_category(const char *title, const AstralRowSpec *rows, size_t count);
void astral_wire_set_decorator(AstralDecorator decorator);
void astral_wire_set_relabeler(AstralRelabeler relabeler);
void astral_wire_set_palette(const uint32_t *from, const uint32_t *to, size_t count);
void astral_wire_set_scales(double radius, double font);
AstralWireStats astral_wire_stats(void);
uint64_t astral_wire_current_channel(void);
int astral_settings_rewrite(const uint8_t *bytes, size_t length, AstralWireBuffer *out);
void astral_wire_buffer_free(AstralWireBuffer *buffer);
int astral_page_push(const AstralPageSpec *page, AstralWireBuffer *buffer);
int astral_page_refresh(const AstralPageSpec *page, AstralWireBuffer *buffer);
int astral_page_pop(AstralWireBuffer *buffer);
int astral_page_depth(void);

#ifdef __cplusplus
}
#endif
