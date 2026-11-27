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
    const char *action;
    int actionPrimary;
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
    kAstralTagIcon,
    kAstralTagText,
} AstralTagShape;

typedef struct {
    char text[32];
    char action_id[96];
    uint32_t background;
    uint32_t foreground;
    uint32_t gradient[3];
    uint32_t gradientCount;
    AstralTagShape shape;
    char icon[512];
    uint32_t tint;
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

typedef struct {
    uint64_t message_id;
    uint64_t user_id;
    uint64_t channel_id;
} AstralMessageBackgroundContext;

typedef enum { AstralReplyAbove = 0, AstralReplyBubble = 1, AstralReplyInside = 2 } AstralReplyStyle;
typedef enum { AstralTimeHeader = 0, AstralTimeInline = 1, AstralTimeHidden = 2 } AstralTimeStyle;

typedef struct {
    uint32_t color;
    double radius;
    int bubble;
    uint8_t reply;
    int nameInside;
    uint8_t time;
} AstralMessageBackgroundStyle;

typedef int (*AstralMessageBackgroundDecorator)(const AstralMessageBackgroundContext *context, AstralMessageBackgroundStyle *style);

void astral_wire_set_message_background_decorator(AstralMessageBackgroundDecorator decorator);

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
    kAstralNameTyping,
} AstralNameKind;

typedef struct {
    AstralNameKind kind;
    const char *text;
    uint64_t user_id;
    uint64_t channel_id;
} AstralNameContext;

typedef int (*AstralNameColorizer)(const AstralNameContext *context, uint32_t *color);

void astral_wire_set_name_colorizer(AstralNameColorizer colorizer);

typedef int (*AstralTypingDecorator)(const AstralNameContext *context, AstralMentionDecoration *decoration);

void astral_wire_set_typing_decorator(AstralTypingDecorator decorator);

typedef struct {
    char action_id[64];
    char label[64];
    char icon[160];
    int danger;
} AstralMenuEntry;

typedef size_t (*AstralMessageMenuProvider)(AstralMenuEntry *entries, size_t capacity);

void astral_wire_set_message_menu_provider(AstralMessageMenuProvider provider);

typedef int (*AstralMessageTextProvider)(uint64_t message_id, char *text, size_t text_capacity, char *note, size_t note_capacity);

void astral_wire_set_message_text_provider(AstralMessageTextProvider provider);

typedef int (*AstralSendTransform)(uint64_t channel_id, const char *text, char *rewritten, size_t capacity);
void astral_wire_set_send_transform(AstralSendTransform transform);
int astral_wire_composer_event(const uint8_t *event, size_t length, AstralWireBuffer *before);
int astral_wire_refresh_message_texts(AstralWireBuffer *buffer);
int astral_wire_refresh_author_tags(AstralWireBuffer *buffer);
int astral_wire_refresh_code_blocks(AstralWireBuffer *buffer);
typedef int (*AstralSpanTransformer)(uint64_t message_id, const uint8_t *spans, size_t length, uint8_t **transformed,
                                     size_t *transformedLength);
void astral_wire_set_span_transformer(AstralSpanTransformer transformer);
int astral_wire_refresh_message_backgrounds(AstralWireBuffer *buffer);

typedef enum {
    AstralPatchBackground = 1 << 0,
    AstralPatchRadius = 1 << 1,
    AstralPatchOpacity = 1 << 2,
    AstralPatchPadding = 1 << 3,
    AstralPatchMargin = 1 << 4,
    AstralPatchBorder = 1 << 5,
    AstralPatchTextColor = 1 << 6,
    AstralPatchHidden = 1 << 7,
    AstralPatchTextScale = 1 << 8,
    AstralPatchGradient = 1 << 9,
    AstralPatchSeeThrough = 1 << 10,
    AstralPatchSelectable = 1 << 11,
    AstralPatchLabelText = 1 << 12,
    AstralPatchLabelColor = 1 << 13,
    AstralPatchLabelTint = 1 << 14
} AstralPatchField;

typedef struct {
    uint32_t fields;
    uint32_t background;
    double radius;
    double opacity;
    float padding[4];
    float margin[4];
    uint32_t borderColor;
    double borderWidth;
    uint32_t textColor;
    double textScale;
    double gradientAngle;
    uint32_t gradientStops[8];
    uint32_t gradientCount;
    char labelText[32];
    uint32_t labelColor;
    uint32_t labelTint;
} AstralNodePatch;

typedef struct {
    const char *identifier;
    const char *type;
    size_t key;
    uint64_t message_id;
    uint64_t author_id;
    uint64_t channel_id;
    const char *parent;
    int32_t node;
    int checked;
    const char *label;
} AstralPatchTarget;

typedef int (*AstralNodePatcher)(const AstralPatchTarget *target, AstralNodePatch *patch);
void astral_wire_set_node_patcher(AstralNodePatcher patcher);
void astral_wire_set_patch_prefixes(const char *const *prefixes, size_t count);
int astral_wire_refresh_node_patches(AstralWireBuffer *buffer);
int astral_wire_take_refresh_busy(void);
uint64_t astral_wire_message_of(const char *identifier);

typedef enum { AstralInsertText = 1, AstralInsertView = 2, AstralInsertImage = 3, AstralInsertPlayer = 4 } AstralInsertKind;

typedef enum {
    AstralInsertColor = 1 << 0,
    AstralInsertBackground = 1 << 1,
    AstralInsertRadius = 1 << 2,
    AstralInsertPadding = 1 << 3,
    AstralInsertMargin = 1 << 4,
    AstralInsertFontSize = 1 << 5,
    AstralInsertWidth = 1 << 6,
    AstralInsertHeight = 1 << 7,
    AstralInsertRow = 1 << 8,
    AstralInsertTint = 1 << 9,
    AstralInsertBold = 1 << 10,
    AstralInsertJustify = 1 << 11,
    AstralInsertFill = 1 << 12,
    AstralInsertMuted = 1 << 13,
    AstralInsertFillHeight = 1 << 14,
    AstralInsertAlign = 1 << 15
} AstralInsertField;

typedef struct {
    AstralInsertKind kind;
    int parent;
    uint32_t fields;
    char text[640];
    char uri[256];
    char tap[96];
    uint32_t color;
    uint32_t background;
    double radius;
    double fontSize;
    double fontWeight;
    double width;
    double height;
    float padding[4];
    float margin[4];
    uint8_t justify;
    uint8_t align;
    double pulseMs;
    double pulseDelayMs;
} AstralInsertNode;

enum { kAstralInsertCapacity = 32 };

typedef int (*AstralNodeInserter)(const AstralPatchTarget *target, AstralInsertNode *nodes, size_t capacity, int *atStart);
void astral_wire_set_node_inserter(AstralNodeInserter inserter);
void astral_wire_set_insert_prefixes(const char *const *prefixes, size_t count);
int astral_wire_refresh_node_inserts(AstralWireBuffer *buffer);
uint64_t astral_wire_message_for_node(int32_t node);
uint64_t astral_wire_message_author(uint64_t message);
size_t astral_wire_composer_text(char *text, size_t capacity);
uint64_t astral_wire_message_for_row(int32_t list, int64_t index);
uint64_t astral_wire_message_at_row(int64_t index);
int astral_wire_scroll_to_message(uint64_t message, int animated, const char *align);
int astral_wire_focus_composer(int focus);
int astral_wire_take_commands(AstralWireBuffer *buffer);
void astral_wire_note_identifiers(const uint8_t *bytes, size_t length);
int astral_wire_identifier(int32_t id, char *out, size_t capacity);
int astral_wire_event_value(const uint8_t *bytes, size_t length, double *value);

typedef int (*AstralEmojiResolver)(const char *codepoints, char *url, size_t capacity);

void astral_wire_set_emoji_resolver(AstralEmojiResolver resolver);
int astral_wire_refresh_images(AstralWireBuffer *buffer, unsigned generation);
typedef int (*AstralImageRewriter)(const char *uri, char *rewritten, size_t capacity);
void astral_wire_set_image_rewriter(AstralImageRewriter rewriter);
void astral_host_calls_set_image_rewriter(AstralImageRewriter rewriter);
int astral_host_call_rewrite(const uint8_t *frame, size_t length, AstralWireBuffer *buffer);
unsigned astral_host_calls_rewritten(void);
int astral_wire_pending_mentions(void);
int astral_wire_retry_mentions(AstralWireBuffer *buffer);
int astral_wire_unsupported_version(void);
unsigned astral_wire_rewrite_failures(void);
int astral_settings_rewrite(const uint8_t *bytes, size_t length, AstralWireBuffer *out);
void astral_wire_buffer_free(AstralWireBuffer *buffer);
int astral_wire_tweens_active(void);
int astral_wire_tween_frame(AstralWireBuffer *buffer, double nowMs);
int astral_page_push(const AstralPageSpec *page, AstralWireBuffer *buffer);
int astral_page_refresh(const AstralPageSpec *page, AstralWireBuffer *buffer);
int astral_page_pop(AstralWireBuffer *buffer);
int astral_page_depth(void);
const char *astral_page_template_status(void);
unsigned astral_page_navigator_count(void);

size_t astral_wire_app_gradient(uint32_t *colors, size_t capacity, double *angle);
int astral_wire_theme_page(AstralWireBuffer *buffer);

#ifdef __cplusplus
}
#endif
