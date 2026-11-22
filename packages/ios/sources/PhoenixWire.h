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
    kAstralSurfaceBackgrounds,
    kAstralSurfaceText,
    kAstralSurfaceAccents,
    kAstralSurfaceCount,
} AstralSurface;

void astral_wire_set_surface_tint(AstralSurface surface, uint32_t color, double saturation);
AstralSurface astral_wire_surface_of(uint32_t color);

typedef enum {
    kAstralRowLink,
    kAstralRowSwitch,
    kAstralRowInfo,
    kAstralRowSwitchLink,
    kAstralRowSelected,
    kAstralRowChoice,
    kAstralRowNumber,
} AstralRowKind;

typedef struct {
    const char *key;
    const char *title;
    const char *subtitle;
    const char *detail;
    const char *icon;
    AstralRowKind kind;
    int on;
    int32_t steps;
    int32_t position;
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
    kAstralLabelChannel,
    kAstralLabelMember,
    kAstralLabelComposer,
    kAstralLabelProfile,
} AstralLabelKind;

typedef struct {
    AstralLabelKind kind;
    uint64_t message_id;
    uint64_t channel_id;
    uint64_t target_id;
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
void astral_wire_set_background_gradient(const uint32_t *colors, size_t count);
void astral_wire_set_tint(uint32_t color, double saturation, double neutral);
int astral_wire_recolor(AstralWireBuffer *buffer);
void astral_wire_forget_colors(void);
unsigned astral_wire_colors_dropped(void);
AstralWireStats astral_wire_stats(void);
uint64_t astral_wire_current_channel(void);

typedef struct {
    uint64_t message_id;
    uint64_t channel_id;
    const char *text;
    uint32_t index;
} AstralMentionContext;

typedef struct {
    char image[512];
    int round;
    int hide_at;
    int after;
    uint32_t color;
} AstralMentionDecoration;

typedef int (*AstralMentionDecorator)(const AstralMentionContext *context, AstralMentionDecoration *decoration);

void astral_wire_set_mention_decorator(AstralMentionDecorator decorator);

typedef int (*AstralMessageTextColorizer)(uint64_t message_id, uint64_t channel_id, uint32_t *color);

void astral_wire_set_message_text_colorizer(AstralMessageTextColorizer colorizer);

typedef enum {
    kAstralNameAvatar,
    kAstralNameHeader,
    kAstralNameVoter,
} AstralNameKind;

typedef struct {
    AstralNameKind kind;
    const char *text;
    uint64_t user_id;
    uint64_t channel_id;
} AstralNameContext;

typedef int (*AstralNameColorizer)(const AstralNameContext *context, uint32_t *color);

void astral_wire_set_name_colorizer(AstralNameColorizer colorizer);
int astral_wire_refresh_images(AstralWireBuffer *buffer, unsigned generation);
int astral_wire_pending_mentions(void);
int astral_wire_retry_mentions(AstralWireBuffer *buffer);
int astral_wire_unsupported_version(void);
unsigned astral_wire_rewrite_failures(void);
int astral_settings_rewrite(const uint8_t *bytes, size_t length, AstralWireBuffer *out);
void astral_wire_buffer_free(AstralWireBuffer *buffer);
int astral_page_push(const AstralPageSpec *page, AstralWireBuffer *buffer);
int astral_page_refresh(const AstralPageSpec *page, AstralWireBuffer *buffer);
int astral_page_pop(AstralWireBuffer *buffer);
int astral_page_depth(void);

void astral_wire_set_settings_backdrop(int enabled, double dim);
int astral_wire_settings_backdrop_enabled(void);
size_t astral_wire_app_gradient(uint32_t *colors, size_t capacity);
int astral_wire_theme_page(AstralWireBuffer *buffer);

#ifdef __cplusplus
}
#endif
