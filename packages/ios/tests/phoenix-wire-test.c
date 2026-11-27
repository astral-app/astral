#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "support/wire-parser.h"
#include "PhoenixWireInternal.h"
#include "support/batch-builder.h"

static char line[1 << 20];
static void sink(void *c, const char *l, size_t n)
{
    (void)c;
    memcpy(line, l, n);
    line[n] = 0;
}

static size_t tagOwner(const AstralDecorationContext *context, AstralTag *tags, size_t capacity)
{
    if (capacity < 1 || context->user_id != 100000000000000001ull || context->channel_id != 100000000000000002ull) return 0;
    strcpy(tags[0].text, "OWNER");
    strcpy(tags[0].action_id, "astral.action.42.2.3");
    tags[0].background = 0xFFED4245;
    tags[0].foreground = 0xFFFFFFFF;
    return 1;
}

static int gMessageBackgroundEnabled = 1;
static size_t gMessageBackgroundCalls;
static uint64_t gMessageBackgroundUser;

static int messageBackground(const AstralMessageBackgroundContext *context, AstralMessageBackgroundStyle *style)
{
    gMessageBackgroundCalls++;
    gMessageBackgroundUser = context->user_id;
    if (!gMessageBackgroundEnabled || context->message_id != 100000000000000003ull || context->channel_id != 100000000000000002ull)
        return 0;
    style->color = 0x806B46E8;
    style->radius = 14;
    return 1;
}

static void messageBackgroundBatch(Batch *batch)
{
    const char *channel = "navigation.channel:100000000000000002";
    const char *author = "message.author:100000000000000003";
    const char *avatar = "message.avatar:100000000000000003";
    batch->length = 0;
    b8(batch, 1);
    create(batch, 799, "view", 2);
    b16(batch, 9);
    b8(batch, 6);
    bstr(batch, channel);
    b16(batch, 13);
    b8(batch, 2);
    create(batch, 800, "view", 3);
    b16(batch, 9);
    b8(batch, 6);
    bstr(batch, "message-row");
    b16(batch, 90);
    b8(batch, 4);
    int64_t originalColor = 0xFF112233;
    memcpy(batch->bytes + batch->length, &originalColor, sizeof(originalColor));
    batch->length += sizeof(originalColor);
    b16(batch, 388);
    b8(batch, 5);
    double radius = 3;
    memcpy(batch->bytes + batch->length, &radius, sizeof(radius));
    batch->length += sizeof(radius);
    create(batch, 801, "text", 1);
    b16(batch, 9);
    b8(batch, 6);
    bstr(batch, author);
    create(batch, 810, "view", 1);
    b16(batch, 9);
    b8(batch, 6);
    bstr(batch, avatar);
    create(batch, 811, "image", 1);
    b16(batch, 542);
    b8(batch, 6);
    bstr(batch, "https://cdn.discordapp.com/avatars/100000000000000001/abc.webp");
    create(batch, 812, "view", 0);
    children(batch, 812, 1, (int[]){811});
    children(batch, 810, 1, (int[]){812});
    children(batch, 800, 2, (int[]){810, 801});
}

static void messageBackgroundTest(void)
{
    AstralWireBuffer out;
    Batch batch;
    astral_wire_set_decorator(NULL);
    astral_wire_set_message_background_decorator(messageBackground);
    gMessageBackgroundEnabled = 1;
    gMessageBackgroundCalls = 0;
    gMessageBackgroundUser = 0;
    messageBackgroundBatch(&batch);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    assert(gMessageBackgroundCalls == 1);
    assert(gMessageBackgroundUser == 100000000000000001ull);
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "[2,800,[[90,4,2154514152],[388,5,14]]]"));
    astral_wire_buffer_free(&out);

    gMessageBackgroundEnabled = 0;
    batch.length = 0;
    b8(&batch, 1);
    b8(&batch, 2);
    b32(&batch, 800);
    b16(&batch, 0);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    assert(gMessageBackgroundCalls == 2);
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "[2,800,[[90,4,4279312947],[388,5,3]]]"));
    astral_wire_buffer_free(&out);
    astral_wire_set_message_background_decorator(NULL);
}

static void decorationTest(void)
{
    AstralWireBuffer out;
    astral_wire_set_decorator(tagOwner);
    Batch b = {.length = 0};
    b8(&b, 1);
    create(&b, 900, "view", 2);
    b16(&b, 13);
    b8(&b, 2);
    b16(&b, 9);
    b8(&b, 6);
    bstr(&b, "navigation.channel:100000000000000002");
    create(&b, 910, "view", 1);
    b16(&b, 9);
    b8(&b, 6);
    bstr(&b, "message.avatar:100000000000000003");
    create(&b, 911, "image", 1);
    b16(&b, 542);
    b8(&b, 6);
    bstr(&b, "https://cdn.discordapp.com/avatars/100000000000000001/abc.webp?size=80");
    create(&b, 912, "stack", 0);
    children(&b, 910, 1, (int[]){912});
    children(&b, 912, 1, (int[]){911});
    create(&b, 920, "view", 0);
    create(&b, 921, "text", 1);
    b16(&b, 9);
    b8(&b, 6);
    bstr(&b, "message.author:100000000000000003");
    create(&b, 922, "view", 0);
    create(&b, 923, "text", 1);
    b16(&b, 9);
    b8(&b, 6);
    bstr(&b, "message.timestamp:100000000000000003");
    children(&b, 920, 3, (int[]){921, 922, 923});
    assert(astral_settings_rewrite(b.bytes, b.length, &out));
    assert(astral_wire_current_channel() == 100000000000000002ull);
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[3,920,[921,922,2063597568,923]]"));
    assert(strstr(line, "[499,6,{\"n\":5"));
    assert(strstr(line, "astral.action.42.2.3"));
    assert(strstr(line, "press"));
    astral_wire_buffer_free(&out);

    Batch again = {.length = 0};
    b8(&again, 1);
    children(&again, 920, 2, (int[]){921, 922});
    assert(astral_settings_rewrite(again.bytes, again.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(!strstr(line, "[4,[2063597568,2063597569]]"));
    assert(strstr(line, "[3,920,[921,922,2063597568]]"));
    astral_wire_buffer_free(&out);

    Batch gone = {.length = 0};
    b8(&gone, 1);
    b8(&gone, 4);
    b32(&gone, 1);
    b32(&gone, 920);
    assert(astral_settings_rewrite(gone.bytes, gone.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[4,[2063597568,2063597569]]"));
    astral_wire_buffer_free(&out);
    astral_wire_set_decorator(NULL);
}

static void cardPageTest(void)
{
    AstralWireBuffer out;
    static const AstralChipSpec chips[] = {{"filter.all", "All 2", 1}, {"filter.enabled", "Enabled 1", 0}};
    static const AstralToolbarSpec toolbar = {chips, 2, "browse", "res://ic_mana_download"};
    static const AstralCardSpec cards[] = {
        {.key = "plugin.dev.astral.tags",
         .title = "More User Tags",
         .authors = "adrian",
         .description = "Adds tags.",
         .version = "v0.0.1",
         .avatars = {"https://cdn.discordapp.com/embed/avatars/0.png"},
         .avatarCount = 1,
         .hasSettings = 1,
         .on = 1},
        {.key = "plugin.dev.astral.broken", .title = "Broken", .error = "start: boom", .failed = 1},
    };
    AstralPageSpec page = {.title = "Plugins", .toolbar = &toolbar, .cards = cards, .cardCount = 2};
    assert(astral_page_push(&page, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[9,6,\"astral.row.filter.all\"]"));
    assert(strstr(line, "[9,6,\"astral.row.browse\"]"));
    assert(strstr(line, "[9,6,\"astral.row.settings.plugin.dev.astral.tags\"]"));
    assert(strstr(line, "[9,6,\"astral.switch.plugin.dev.astral.tags\"]"));
    assert(strstr(line, "[322,5,0.5]"));
    assert(strstr(line, "[9,6,\"astral.row.more.plugin.dev.astral.broken\"]"));
    assert(!strstr(line, "astral.row.settings.plugin.dev.astral.broken"));
    astral_wire_buffer_free(&out);
    assert(astral_page_refresh(&page, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    astral_wire_buffer_free(&out);
    assert(astral_page_pop(&out));
    astral_wire_buffer_free(&out);

    static const AstralCardSpec market[] = {
        {.key = "plugin.dev.astral.tags", .title = "More User Tags", .action = "Get", .actionPrimary = 1},
    };
    AstralPageSpec store = {.title = "Marketplace", .cards = market, .cardCount = 1};
    assert(astral_page_push(&store, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[9,6,\"astral.row.plugin.dev.astral.tags\"]"));
    assert(!strstr(line, "astral.switch.plugin.dev.astral.tags"));
    astral_wire_buffer_free(&out);
    assert(astral_page_pop(&out));
    astral_wire_buffer_free(&out);
}

static void categoryRowsTest(void)
{
    AstralWireBuffer out;
    Batch gone = {.length = 0};
    b8(&gone, 1);
    b8(&gone, 4);
    b32(&gone, 1);
    b32(&gone, 10);
    assert(astral_settings_rewrite(gone.bytes, gone.length, &out));
    astral_wire_buffer_free(&out);
    static const AstralRowSpec rows[] = {
        {.key = "general", .title = "General", .icon = "res://ic_mana_settings"},
        {.key = "plugins", .title = "Plugins", .icon = "res://ic_mana_puzzle_piece"},
        {.key = "design", .title = "Design", .icon = "res://ic_mana_paintbrush_thick"},
    };
    astral_wire_set_category("astral", rows, 3);
    Batch batch;
    overview(&batch);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[9,6,\"settings.row.astral.general\"]"));
    assert(strstr(line, "[9,6,\"settings.row.astral.plugins\"]"));
    assert(strstr(line, "[9,6,\"settings.row.astral.design\"]"));
    assert(strstr(line, "[3,10,[11,2046820352,20]]"));
    astral_wire_buffer_free(&out);
}

static void gradientTagTest(void)
{
    AstralTag tag = {.text = "ADMIN", .foreground = 0xFF000000, .gradient = {0xFF4CAD50, 0xFFB2F8FF}, .gradientCount = 2};
    Out out = {0};
    Decorated slot = {.parent = 1};
    wirePut8(&out, 1);
    wireBuildTag(&out, &tag, &slot);
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[91,8,"));
    assert(!strstr(line, "[90,4,"));
    free(out.bytes);
}

static void paletteTest(void)
{
    AstralWireBuffer out;
    uint32_t from[] = {0xFF5865F2}, to[] = {0xFF9B6BFF};
    astral_wire_set_palette(from, to, 1);
    Batch b = {.length = 0};
    b8(&b, 1);
    create(&b, 950, "view", 2);
    b16(&b, 90);
    b8(&b, 4);
    b32(&b, 0xFF5865F2);
    b32(&b, 0);
    b16(&b, 129);
    b8(&b, 3);
    b32(&b, 0xFF000000);
    assert(astral_settings_rewrite(b.bytes, b.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[90,4,4288375807]"));
    assert(strstr(line, "[129,3,-16777216]") || strstr(line, "[129,3,4278190080]"));
    astral_wire_buffer_free(&out);
    astral_wire_set_palette(NULL, NULL, 0);
}

static int labelWithUser(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity)
{
    (void)length;
    const char *user = context->kind == kAstralLabelReply ? "@replied" : context->kind == kAstralLabelChannel ? "#chan" : "@author";
    return snprintf(out, capacity, "%s (%s)", text, user);
}

static void relabelTest(void)
{
    AstralWireBuffer out;
    astral_wire_set_relabeler(labelWithUser);
    Batch b = {.length = 0};
    b8(&b, 1);
    create(&b, 970, "view", 1);
    b16(&b, 9);
    b8(&b, 6);
    bstr(&b, "message-row");
    create(&b, 971, "text", 2);
    b16(&b, 1);
    b8(&b, 7);
    b32(&b, 1);
    b8(&b, 6);
    bstr(&b, "replyPreview");
    b16(&b, 468);
    b8(&b, 7);
    b32(&b, 1);
    b8(&b, 8);
    b32(&b, 1);
    b16(&b, 499);
    b8(&b, 6);
    bstr(&b, "Rae");
    create(&b, 972, "text", 2);
    b16(&b, 9);
    b8(&b, 6);
    bstr(&b, "message.author:100000000000000009");
    b16(&b, 499);
    b8(&b, 6);
    bstr(&b, "Kode");
    create(&b, 973, "view", 0);
    children(&b, 973, 1, (int[]){971});
    children(&b, 970, 2, (int[]){973, 972});
    assert(astral_settings_rewrite(b.bytes, b.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[499,6,{\"n\":14"));
    assert(strstr(line, "\"m\":[[499,[6,{\"n\":14"));
    astral_wire_buffer_free(&out);

    uint32_t from[] = {0xFF000000}, to[] = {0xFF111111};
    astral_wire_set_palette(from, to, 1);
    Batch update = {.length = 0};
    b8(&update, 1);
    b8(&update, 2);
    b32(&update, 972);
    b16(&update, 2);
    b16(&update, 499);
    b8(&update, 6);
    bstr(&update, "Kode");
    b16(&update, 129);
    b8(&update, 3);
    b32(&update, 0xFF000000);
    assert(astral_settings_rewrite(update.bytes, update.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[499,6,{\"n\":14"));
    assert(strstr(line, "[129,3,-15658735]") || strstr(line, "[129,3,4279308561]"));
    astral_wire_buffer_free(&out);
    astral_wire_set_palette(NULL, NULL, 0);

    Batch channels = {.length = 0};
    b8(&channels, 1);
    create(&channels, 990, "view", 1);
    b16(&channels, 9);
    b8(&channels, 6);
    bstr(&channels, "navigation.channel:100000000000000002");
    create(&channels, 991, "view", 0);
    create(&channels, 992, "text", 1);
    b16(&channels, 499);
    b8(&channels, 6);
    bstr(&channels, "dev");
    children(&channels, 991, 1, (int[]){992});
    children(&channels, 990, 1, (int[]){991});
    assert(astral_settings_rewrite(channels.bytes, channels.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[499,6,{\"n\":11"));
    astral_wire_buffer_free(&out);
    astral_wire_set_relabeler(NULL);
}

static uint64_t gProfileUser;

static int profileRelabeler(const AstralLabelContext *context, const char *text, size_t length, char *out, size_t capacity)
{
    if (context->kind != kAstralLabelProfile) return 0;
    gProfileUser = context->target_id;
    return snprintf(out, capacity, "%.*s (@renamed)", (int)length, text);
}

static void profileRelabelTest(void)
{
    Batch batch = {.length = 0};
    b8(&batch, 1);
    create(&batch, 700, "scroll", 1);
    b16(&batch, 9);
    b8(&batch, 6);
    bstr(&batch, "profile-scroll");
    create(&batch, 701, "image", 1);
    b16(&batch, 542);
    b8(&batch, 6);
    bstr(&batch, "https://cdn.discordapp.com/avatars/123456789012345678/a.png?size=256");
    create(&batch, 702, "text", 2);
    b16(&batch, 499);
    b8(&batch, 6);
    bstr(&batch, "Small");
    b16(&batch, 207);
    b8(&batch, 5);
    double small = 14;
    memcpy(batch.bytes + batch.length, &small, 8);
    batch.length += 8;
    create(&batch, 703, "text", 2);
    b16(&batch, 499);
    b8(&batch, 6);
    bstr(&batch, "Display Name");
    b16(&batch, 207);
    b8(&batch, 5);
    double large = 24;
    memcpy(batch.bytes + batch.length, &large, 8);
    batch.length += 8;
    children(&batch, 700, 3, (int[]){701, 702, 703});
    astral_wire_set_relabeler(profileRelabeler);
    AstralWireBuffer out;
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    assert(memmem(out.bytes, out.length, "Display Name (@renamed)", 23));
    assert(!memmem(out.bytes, out.length, "Small (@renamed)", 16));
    assert(gProfileUser == 123456789012345678ull);
    astral_wire_buffer_free(&out);
    astral_wire_set_relabeler(NULL);
}

static void gradientBatch(Batch *batch, int id)
{
    batch->length = 0;
    b8(batch, 1);
    create(batch, id, "view", 1);
    b16(batch, 91);
    b8(batch, 8);
    b32(batch, 2);
    b16(batch, 70);
    b8(batch, 5);
    double angle = 90;
    memcpy(batch->bytes + batch->length, &angle, 8);
    batch->length += 8;
    b16(batch, 577);
    b8(batch, 7);
    b32(batch, 2);
    b8(batch, 3);
    b32(batch, (int)0xFF111111);
    b8(batch, 3);
    b32(batch, (int)0xFF222222);
}

static void gradientThemeTest(void)
{
    AstralWireBuffer out;
    Batch batch = {.length = 0};
    const uint32_t from[] = {0xFF111111}, to[] = {0xFFAA0000};
    astral_wire_set_palette(from, to, 1);
    gradientBatch(&batch, 800);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    uint32_t mapped = 0xFFAA0000, kept = 0xFF222222;
    assert(memmem(out.bytes, out.length, &mapped, 4) && memmem(out.bytes, out.length, &kept, 4));
    astral_wire_buffer_free(&out);
    astral_wire_set_palette(NULL, NULL, 0);

    const uint32_t stops[] = {0xFF0000AA, 0xFF00AA00, 0xFFAA00AA};
    astral_wire_set_background_gradient(stops, 3);
    gradientBatch(&batch, 801);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    uint32_t first = 0xFF000022, last = 0xFF440044, old = 0xFF111111;
    assert(memmem(out.bytes, out.length, &first, 4) && memmem(out.bytes, out.length, &last, 4) && !memmem(out.bytes, out.length, &old, 4));
    astral_wire_buffer_free(&out);
    gradientBatch(&batch, 0x7b000010);
    if (astral_settings_rewrite(batch.bytes, batch.length, &out)) {
        assert(memmem(out.bytes, out.length, &old, 4));
        astral_wire_buffer_free(&out);
    }
    astral_wire_set_background_gradient(NULL, 0);
}

static void coloredView(Batch *batch, int id, uint32_t color)
{
    create(batch, id, "view", 1);
    b16(batch, 90);
    b8(batch, 4);
    b32(batch, (int)color);
    b32(batch, 0);
}

static void recolorTest(void)
{
    AstralWireBuffer out;
    astral_wire_forget_colors();
    assert(!astral_wire_recolor(&out));
    Batch batch = {.length = 0};
    b8(&batch, 1);
    coloredView(&batch, 960, 0xFF5865F2);
    coloredView(&batch, 961, 0xFF123456);
    coloredView(&batch, 0x7a000500, 0xFF5865F2);
    if (astral_settings_rewrite(batch.bytes, batch.length, &out)) astral_wire_buffer_free(&out);
    uint32_t from[] = {0xFF5865F2}, to[] = {0xFF9B6BFF};
    astral_wire_set_palette(from, to, 1);
    assert(astral_wire_recolor(&out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[2,960,[[90,4,4288375807]]]"));
    assert(strstr(line, "[2,961,[[90,4,4279383126]]]"));
    assert(!strstr(line, "2046821632"));
    astral_wire_buffer_free(&out);
    astral_wire_set_palette(NULL, NULL, 0);
    assert(astral_wire_recolor(&out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[2,960,[[90,4,4283983346]]]"));
    astral_wire_buffer_free(&out);
    Batch removal = {.length = 0};
    b8(&removal, 1);
    b8(&removal, 4);
    b32(&removal, 2);
    b32(&removal, 960);
    b32(&removal, 961);
    if (astral_settings_rewrite(removal.bytes, removal.length, &out)) astral_wire_buffer_free(&out);
    assert(!astral_wire_recolor(&out));
}

static void putDouble(Batch *batch, double value)
{
    memcpy(batch->bytes + batch->length, &value, 8);
    batch->length += 8;
}

static void liveThemeTest(void)
{
    AstralWireBuffer out;
    astral_wire_forget_colors();
    astral_wire_set_palette(NULL, NULL, 0);
    Batch batch = {.length = 0};
    b8(&batch, 1);
    create(&batch, 970, "text", 1);
    b16(&batch, 207);
    b8(&batch, 5);
    putDouble(&batch, 14);
    create(&batch, 971, "view", 1);
    b16(&batch, 91);
    b8(&batch, 8);
    b32(&batch, 2);
    b16(&batch, 70);
    b8(&batch, 5);
    putDouble(&batch, 90);
    b16(&batch, 577);
    b8(&batch, 7);
    b32(&batch, 2);
    b8(&batch, 3);
    b32(&batch, (int)0xFF111111);
    b8(&batch, 3);
    b32(&batch, (int)0xFF222222);
    if (astral_settings_rewrite(batch.bytes, batch.length, &out)) astral_wire_buffer_free(&out);
    astral_wire_set_scales(1, 2);
    const uint32_t stops[] = {0xFF0000AA, 0xFFAA00AA};
    astral_wire_set_background_gradient(stops, 2);
    assert(astral_wire_recolor(&out) == 2);
    double doubled = 28, original = 14;
    uint32_t first = 0xFF000022, old = 0xFF111111;
    assert(memmem(out.bytes, out.length, &doubled, 8) && memmem(out.bytes, out.length, &first, 4) &&
           !memmem(out.bytes, out.length, &old, 4));
    astral_wire_buffer_free(&out);
    astral_wire_set_scales(1, 1);
    astral_wire_set_background_gradient(NULL, 0);
    assert(astral_wire_recolor(&out) == 2);
    assert(memmem(out.bytes, out.length, &original, 8) && memmem(out.bytes, out.length, &old, 4));
    astral_wire_buffer_free(&out);
    assert(astral_wire_colors_dropped() == 0);
    astral_wire_forget_colors();
}

static void trackerOverflowTest(void)
{
    AstralWireBuffer out;
    astral_wire_forget_colors();
    static Batch batch;
    int id = 1000;
    for (int round = 0; round < 12; round++) {
        batch.length = 0;
        b8(&batch, 1);
        for (int index = 0; index < 2500; index++) {
            create(&batch, id++, "v", 1);
            b16(&batch, 90);
            b8(&batch, 3);
            b32(&batch, (int)0xFF101010);
        }
        if (astral_settings_rewrite(batch.bytes, batch.length, &out)) astral_wire_buffer_free(&out);
    }
    assert(astral_wire_colors_dropped() > 0);
    astral_wire_forget_colors();
    assert(astral_wire_colors_dropped() == 0);
}

static void listRowTest(void)
{
    AstralWireBuffer out;
    Batch batch = {.length = 0};
    b8(&batch, 1);
    create(&batch, 1500, "list", 1);
    b16(&batch, 194);
    b8(&batch, 3);
    b32(&batch, 2);
    create(&batch, 1501, "view", 1);
    b16(&batch, 9);
    b8(&batch, 6);
    bstr(&batch, "message-row");
    create(&batch, 1502, "text", 1);
    b16(&batch, 9);
    b8(&batch, 6);
    bstr(&batch, "message.content:100000000000000009");
    create(&batch, 1503, "view", 0);
    children(&batch, 1501, 1, (int[]){1502});
    children(&batch, 1500, 2, (int[]){1503, 1501});
    if (astral_settings_rewrite(batch.bytes, batch.length, &out)) astral_wire_buffer_free(&out);
    assert(astral_wire_message_for_row(1500, 3) == 100000000000000009ull);
    assert(astral_wire_message_for_row(1500, 2) == 0);
    assert(astral_wire_message_for_row(1500, 1) == 0);
    assert(astral_wire_message_for_row(1500, 4) == 0);
    assert(astral_wire_message_for_node(1502) == 100000000000000009ull);

    Batch scrolled = {.length = 0};
    b8(&scrolled, 1);
    b8(&scrolled, 2);
    b32(&scrolled, 1500);
    b16(&scrolled, 2);
    b16(&scrolled, 194);
    b8(&scrolled, 3);
    b32(&scrolled, 9);
    b16(&scrolled, 136);
    b8(&scrolled, 3);
    b32(&scrolled, 6);
    if (astral_settings_rewrite(scrolled.bytes, scrolled.length, &out)) astral_wire_buffer_free(&out);
    assert(astral_wire_message_for_row(1500, 5) == 100000000000000009ull);
    assert(astral_wire_message_for_row(1500, 9) == 0);

    assert(astral_wire_scroll_to_message(100000000000000009ull, 1, "visible"));
    assert(!astral_wire_scroll_to_message(100000000000000008ull, 1, "visible"));
    AstralWireBuffer commands;
    assert(astral_wire_take_commands(&commands));
    int32_t list = 0, index = 0;
    memcpy(&list, commands.bytes + 2, 4);
    memcpy(&index, commands.bytes + 2 + 4 + 1 + 4 + 2 + 1, 4);
    assert(commands.bytes[0] == 1 && commands.bytes[1] == 7 && commands.bytes[6] == 8 && list == 1500 && index == 5);
    astral_wire_buffer_free(&commands);
    assert(!astral_wire_take_commands(&commands));
}

static int cleanLinks(uint64_t channel, const char *text, char *rewritten, size_t capacity)
{
    (void)channel;
    const char *tracking = strstr(text, "?utm=1");
    if (!tracking) return 0;
    snprintf(rewritten, capacity, "%.*s", (int)(tracking - text), text);
    return 1;
}

static void composerBatch(Batch *batch, const char *value)
{
    batch->length = 0;
    b8(batch, 1);
    create(batch, 1600, "input", 2);
    b16(batch, 9);
    b8(batch, 6);
    bstr(batch, "composer");
    b16(batch, 543);
    b8(batch, 6);
    bstr(batch, value);
    create(batch, 1601, "view", 1);
    b16(batch, 9);
    b8(batch, 6);
    bstr(batch, "send");
}

static void composerEvent(Batch *event, int node, int kind, const char *text, int count)
{
    event->length = 0;
    b8(event, 1);
    b8(event, 1);
    b32(event, node);
    b16(event, kind);
    b8(event, 8);
    b32(event, text ? 2 : 0);
    if (!text) return;
    b16(event, 499);
    b8(event, 6);
    bstr(event, text);
    b16(event, 811);
    b8(event, 3);
    b32(event, count);
}

static void composerSendTest(void)
{
    AstralWireBuffer out;
    Batch batch, event;
    astral_wire_set_send_transform(cleanLinks);
    composerBatch(&batch, "see https://a.test/x?utm=1");
    if (astral_settings_rewrite(batch.bytes, batch.length, &out)) astral_wire_buffer_free(&out);
    composerEvent(&event, 1600, 576, "see https://a.test/x?utm=1", 4);
    assert(!astral_wire_composer_event(event.bytes, event.length, &out));
    composerEvent(&event, 1601, 614, NULL, 0);
    assert(astral_wire_composer_event(event.bytes, event.length, &out));
    assert(out.length > 8 && out.bytes[6] == (576 & 0xff));
    assert(memmem(out.bytes, out.length, "see https://a.test/x", 20));
    assert(!memmem(out.bytes, out.length, "utm", 3));
    int32_t count;
    memcpy(&count, out.bytes + out.length - 4, 4);
    assert(count == 5);
    astral_wire_buffer_free(&out);

    composerBatch(&batch, "");
    if (astral_settings_rewrite(batch.bytes, batch.length, &out)) astral_wire_buffer_free(&out);
    composerEvent(&event, 1601, 614, NULL, 0);
    assert(!astral_wire_composer_event(event.bytes, event.length, &out));
    astral_wire_set_send_transform(NULL);
}

static void fadeOverlayTest(void)
{
    AstralWireBuffer out;
    Batch batch = {.length = 0};
    b8(&batch, 1);
    create(&batch, 980, "view", 1);
    b16(&batch, 91);
    b8(&batch, 8);
    b32(&batch, 1);
    b16(&batch, 577);
    b8(&batch, 7);
    b32(&batch, 2);
    b8(&batch, 3);
    b32(&batch, 0x00041B2E);
    b8(&batch, 3);
    b32(&batch, (int)0xFF041B2E);
    const uint32_t stops[] = {0xFF3B2380, 0xFF120B2E};
    astral_wire_set_background_gradient(stops, 2);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    long long first = 0, second = 0;
    const char *found = strstr(line, "[577,[7,[[3,");
    assert(found && sscanf(found, "[577,[7,[[3,%lld],[3,%lld]", &first, &second) == 2);
    uint32_t transparent = (uint32_t)first, opaque = (uint32_t)second;
    assert((transparent >> 24) == 0 && (opaque >> 24) == 0xFF);
    unsigned red = (opaque >> 16) & 0xFF, green = (opaque >> 8) & 0xFF, blue = opaque & 0xFF;
    assert(red > green && blue > green && red + green + blue < 3 * 64);
    astral_wire_buffer_free(&out);
    astral_wire_set_background_gradient(NULL, 0);
    astral_wire_forget_colors();
}

static void tintTest(void)
{
    AstralWireBuffer out;
    astral_wire_set_palette(NULL, NULL, 0);
    astral_wire_set_tint(0xFF8B5CF6, 0.45, 0.3);
    Batch batch = {.length = 0};
    b8(&batch, 1);
    coloredView(&batch, 990, 0xFF2B2D31);
    coloredView(&batch, 991, 0xFF23A55A);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    uint32_t grey = 0xFF2B2D31, green = 0xFF23A55A;
    assert(!memmem(out.bytes, out.length, &grey, 4) && memmem(out.bytes, out.length, &green, 4));
    astral_wire_buffer_free(&out);
    astral_wire_set_tint(0, 0, 0);
    astral_wire_forget_colors();
}

static void surfaceTintTest(void)
{
    AstralWireBuffer out;
    astral_wire_set_palette(NULL, NULL, 0);
    astral_wire_set_tint(0, 0, 0);
    assert(astral_wire_surface_of(0xFF2B2D31) == kAstralSurfaceBackgrounds);
    assert(astral_wire_surface_of(0xFFDBDEE1) == kAstralSurfaceText);
    assert(astral_wire_surface_of(0xFF5865F2) == kAstralSurfaceAccents);
    astral_wire_set_surface_tint(kAstralSurfaceAccents, 0xFFF59E0B, 0.9);
    Batch batch = {.length = 0};
    b8(&batch, 1);
    coloredView(&batch, 992, 0xFF2B2D31);
    coloredView(&batch, 993, 0xFF5865F2);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    uint32_t grey = 0xFF2B2D31, blurple = 0xFF5865F2;
    assert(memmem(out.bytes, out.length, &grey, 4) && !memmem(out.bytes, out.length, &blurple, 4));
    astral_wire_buffer_free(&out);
    astral_wire_set_tint(0, 0, 0);
    astral_wire_forget_colors();
}

static void unsupportedVersionTest(void)
{
    AstralWireBuffer out;
    Batch future = {.length = 0};
    b8(&future, 2);
    children(&future, 5, 1, (int[]){6});
    assert(!astral_wire_unsupported_version());
    assert(!astral_settings_rewrite(future.bytes, future.length, &out));
    assert(astral_wire_unsupported_version() == 2);
    Batch current = {.length = 0};
    b8(&current, 1);
    children(&current, 5, 1, (int[]){6});
    assert(!astral_settings_rewrite(current.bytes, current.length, &out));
}

static int gPatchMode;

static int patchNode(const AstralPatchTarget *target, AstralNodePatch *patch)
{
    if (strcmp(target->identifier, "demo.card:42") != 0 || strcmp(target->type, "view") != 0) return 0;
    assert(target->key == strlen("demo.card:") && target->message_id == 0);
    if (gPatchMode == 1) {
        patch->fields = AstralPatchBackground | AstralPatchRadius;
        patch->background = 0xFF5865F2;
        patch->radius = 6;
    } else if (gPatchMode == 2) {
        patch->fields = AstralPatchHidden;
    }
    return gPatchMode ? 1 : 0;
}

static void nodePatchTest(void)
{
    AstralWireBuffer out;
    Batch batch = {.length = 0};
    const char *prefixes[] = {"demo.card:"};
    astral_wire_set_patch_prefixes(prefixes, 1);
    astral_wire_set_node_patcher(patchNode);
    gPatchMode = 1;
    b8(&batch, 1);
    create(&batch, 900, "view", 2);
    b16(&batch, 9);
    b8(&batch, 6);
    bstr(&batch, "demo.card:42");
    b16(&batch, 90);
    b8(&batch, 4);
    int64_t original = 0xFF101010;
    memcpy(batch.bytes + batch.length, &original, sizeof(original));
    batch.length += sizeof(original);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "[2,900,[[90,4,4283983346],[388,5,6]]]"));
    astral_wire_buffer_free(&out);

    gPatchMode = 2;
    assert(astral_wire_refresh_node_patches(&out));
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "[2,900,[[90,4,4279242768],[388,0,null],[322,5,0]"));
    astral_wire_buffer_free(&out);

    gPatchMode = 0;
    assert(astral_wire_refresh_node_patches(&out));
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "[2,900,[[322,0,null],[332,0,null],[287,0,null],[553,0,null],[235,0,null],[123,0,null]]]"));
    astral_wire_buffer_free(&out);
    assert(astral_wire_refresh_node_patches(&out) == 0);
    astral_wire_set_node_patcher(NULL);
    astral_wire_set_patch_prefixes(NULL, 0);
}

static int hideLateCard(const AstralPatchTarget *target, AstralNodePatch *patch)
{
    if (strcmp(target->identifier, "late.card:7") != 0 || !gPatchMode) return 0;
    patch->fields = AstralPatchHidden;
    return 1;
}

static void latePrefixPatchTest(void)
{
    AstralWireBuffer out;
    Batch batch = {.length = 0};
    astral_wire_set_node_patcher(hideLateCard);
    gPatchMode = 2;
    b8(&batch, 1);
    create(&batch, 901, "view", 1);
    b16(&batch, 9);
    b8(&batch, 6);
    bstr(&batch, "late.card:7");
    astral_settings_rewrite(batch.bytes, batch.length, &out);
    astral_wire_buffer_free(&out);
    astral_wire_refresh_node_patches(&out);
    astral_wire_buffer_free(&out);
    const char *prefixes[] = {"late.card:"};
    astral_wire_set_patch_prefixes(prefixes, 1);
    assert(astral_wire_refresh_node_patches(&out));
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "[2,901,["));
    assert(strstr(line, "[322,5,0]"));
    astral_wire_buffer_free(&out);
    gPatchMode = 0;
    astral_wire_refresh_node_patches(&out);
    astral_wire_buffer_free(&out);
    astral_wire_set_node_patcher(NULL);
    astral_wire_set_patch_prefixes(NULL, 0);
}

static int insertNodes(const AstralPatchTarget *target, AstralInsertNode *nodes, size_t capacity, int *atStart)
{
    assert(capacity >= 2);
    if (strcmp(target->identifier, "demo.slot:1") != 0) return 0;
    *atStart = 0;
    nodes[0] = (AstralInsertNode){
        .kind = AstralInsertView, .parent = -1, .fields = AstralInsertRow | AstralInsertBackground, .background = 0xFF202020};
    strcpy(nodes[0].tap, "astral.tap.1:open:1");
    nodes[1] = (AstralInsertNode){.kind = AstralInsertText, .parent = 0, .fields = AstralInsertColor, .color = 0xFFFFFFFF};
    strcpy(nodes[1].text, "Hi");
    return 2;
}

static void nodeInsertTest(void)
{
    AstralWireBuffer out;
    Batch batch = {.length = 0};
    const char *prefixes[] = {"demo.slot:"};
    astral_wire_set_insert_prefixes(prefixes, 1);
    astral_wire_set_node_inserter(insertNodes);
    b8(&batch, 1);
    create(&batch, 950, "view", 1);
    b16(&batch, 9);
    b8(&batch, 6);
    bstr(&batch, "demo.slot:1");
    create(&batch, 951, "text", 0);
    children(&batch, 950, 1, (int[]){951});
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    char *created = strstr(line, "\"view\",[[9,6,\"astral.tap.1:open:1\"]");
    if (!created) created = strstr(line, "astral.tap.1:open:1");
    assert(created);
    assert(strstr(line, "[499,6,{\"n\":2,"));
    char needle[64];
    int32_t inserted = 0;
    char *list = strstr(line, "[3,950,[951,");
    assert(list && sscanf(list, "[3,950,[951,%d]]", &inserted) == 1 && inserted > 0);
    astral_wire_buffer_free(&out);

    batch.length = 0;
    b8(&batch, 1);
    create(&batch, 952, "text", 0);
    children(&batch, 950, 2, (int[]){951, 952});
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    snprintf(needle, sizeof(needle), "[3,950,[951,952,%d]]", inserted);
    assert(strstr(line, needle));
    astral_wire_buffer_free(&out);
    astral_wire_set_node_inserter(NULL);
    astral_wire_set_insert_prefixes(NULL, 0);
}

static void childEdit(Batch *batch, int parent, int keep, int drop, int added, const int *ids)
{
    b8(batch, 10);
    b32(batch, parent);
    b32(batch, 1);
    b32(batch, keep);
    b32(batch, drop);
    b32(batch, added);
    for (int index = 0; index < added; index++) b32(batch, ids[index]);
}

static void childEditsTest(void)
{
    AstralWireBuffer out;
    Batch batch = {.length = 0};
    b8(&batch, 1);
    children(&batch, 2000, 3, (int[]){2001, 2002, 2003});
    if (astral_settings_rewrite(batch.bytes, batch.length, &out)) astral_wire_buffer_free(&out);

    Batch edit = {.length = 0};
    b8(&edit, 1);
    childEdit(&edit, 2000, 1, 1, 1, (int[]){2009});
    assert(astral_settings_rewrite(edit.bytes, edit.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[3,2000,[2001,2009,2003]]"));
    astral_wire_buffer_free(&out);

    Batch append = {.length = 0};
    b8(&append, 1);
    childEdit(&append, 2000, 3, 0, 1, (int[]){2004});
    assert(astral_settings_rewrite(append.bytes, append.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[3,2000,[2001,2009,2003,2004]]"));
    astral_wire_buffer_free(&out);

    Batch overflow = {.length = 0};
    b8(&overflow, 1);
    childEdit(&overflow, 2000, 9, 0, 0, NULL);
    assert(!astral_settings_rewrite(overflow.bytes, overflow.length, &out));
    const int32_t *known = NULL;
    assert(wireDiscordChildren(2000, &known) == 0);
}

int main(void)
{
    childEditsTest();
    messageBackgroundTest();
    nodePatchTest();
    latePrefixPatchTest();
    decorationTest();
    gradientTagTest();
    relabelTest();
    paletteTest();
    recolorTest();
    surfaceTintTest();
    liveThemeTest();
    trackerOverflowTest();
    listRowTest();
    composerSendTest();
    fadeOverlayTest();
    tintTest();
    Batch batch;
    overview(&batch);
    AstralWireBuffer out;
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[3,10,[11,2046820352,20]]"));
    assert(strstr(line, "[1,2046820355,\"view\",[[9,6,\"settings.row.astral\"],[11,6,{\"n\":6"));
    char *clone = strstr(line, "[1,2046820355,");
    char *next = strstr(clone + 1, "[1,2046820356,");
    assert(clone && next);
    *next = 0;
    assert(strstr(clone, "[320,7"));
    assert(strstr(clone, "[389,5,12],[390,5,12],[391,5,12],[392,5,12]"));
    assert(strstr(clone, "[733,11,{\"words\":2}]"));
    *next = '[';
    assert(strstr(line, "[542,6,{\"n\":22"));
    astral_wire_buffer_free(&out);

    Batch update = {.length = 0};
    b8(&update, 1);
    children(&update, 10, 3, (int[]){11, 20, 30});
    assert(astral_settings_rewrite(update.bytes, update.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[3,10,[11,2046820352,20,30]]"));
    astral_wire_buffer_free(&out);

    Batch unrelated = {.length = 0};
    b8(&unrelated, 1);
    b8(&unrelated, 2);
    b32(&unrelated, 5);
    b16(&unrelated, 0);
    assert(!astral_settings_rewrite(unrelated.bytes, unrelated.length, &out));

    Batch close = {.length = 0};
    b8(&close, 1);
    b8(&close, 4);
    b32(&close, 2);
    b32(&close, 10);
    b32(&close, 11);
    assert(astral_settings_rewrite(close.bytes, close.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[4,[2046820352,2046820353,2046820354,2046820355,2046820356,2046820357,2046820358,2046820359,2046820360]]"));
    astral_wire_buffer_free(&out);

    overview(&batch);
    assert(astral_settings_rewrite(batch.bytes, batch.length, &out));
    astral_wire_buffer_free(&out);

    static const AstralRowSpec pluginRows[] = {
        {.key = "plugin.dev.astral.tint", .title = "Cyan Text", .icon = "res://ic_mana_puzzle_piece", .kind = kAstralRowSwitch, .on = 1},
    };
    static const AstralRowSpec developerRows[] = {
        {.key = "style-test",
         .title = "Text style test",
         .subtitle = "Shows a sample",
         .icon = "res://ic_mana_wrench",
         .kind = kAstralRowSwitch},
        {.key = "engine", .title = "Engine", .detail = "Phoenix", .icon = "res://ic_mana_bug", .kind = kAstralRowInfo},
        {.key = "more", .title = "More", .kind = kAstralRowLink},
    };
    static const AstralSectionSpec sections[] = {
        {"Plugins", pluginRows, 1},
        {"Developer", developerRows, 3},
    };
    AstralPageSpec page = {.title = "astral", .sections = sections, .count = 2};

    assert(astral_page_push(&page, &out));
    assert(astral_page_depth() == 1);
    assert(astral_wire_summarise(out.bytes, out.length, sink, NULL) > 0);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[3,5,[6,100,2046820608]]"));
    assert(strstr(line, "[2,5,[[252,3,2]]]"));
    assert(strstr(line, "[9,6,\"astral.back\"]"));
    assert(strstr(line, "[9,6,\"astral.row.plugin.dev.astral.tint\"]"));
    assert(strstr(line, "[9,6,\"astral.switch.plugin.dev.astral.tint\"]"));
    assert(strstr(line, "[9,6,\"astral.switch.style-test\"]"));
    assert(strstr(line, "[414,6,\"switch\"]"));
    assert(strstr(line, "[90,4,4283983346]"));
    assert(strstr(line, "[90,3,520093696]"));
    assert(strstr(line, "[535,5,28]"));
    assert(strstr(line, "[535,5,0]"));
    assert(!strstr(line, "[740,12"));
    assert(strstr(line, "[499,6,{\"n\":14"));
    assert(strstr(line, "[322,5,0.62]"));
    astral_wire_buffer_free(&out);

    assert(astral_page_push(&page, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[3,5,[6,100,2046820608,2047082752]]"));
    assert(strstr(line, "[2,5,[[252,3,3]]]"));
    assert(astral_page_depth() == 2);
    astral_wire_buffer_free(&out);
    assert(astral_page_pop(&out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[2,5,[[252,3,2]]]"));
    assert(strstr(line, "[3,5,[6,100,2046820608]]"));
    assert(strstr(line, "[4,[2047082752,"));
    assert(astral_page_depth() == 1);
    astral_wire_buffer_free(&out);

    AstralRowSpec flipped[] = {
        {.key = "plugin.dev.astral.tint", .title = "Cyan Text", .icon = "res://ic_mana_puzzle_piece", .kind = kAstralRowSwitch}};
    AstralSectionSpec refreshed[] = {{"Plugins", flipped, 1}};
    AstralPageSpec refreshedPage = {.title = "astral", .sections = refreshed, .count = 1};
    assert(astral_page_refresh(&refreshedPage, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "\"complete\":true"));
    assert(strstr(line, "[4,["));
    assert(strstr(line, "[90,3,520093696]"));
    assert(!strstr(line, "[90,4,4283983346]"));
    astral_wire_buffer_free(&out);

    assert(astral_page_pop(&out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[2,5,[[252,3,1]]]"));
    assert(strstr(line, "[3,5,[6,100]]"));
    assert(strstr(line, "[4,[2046820608,"));
    assert(astral_page_depth() == 0);
    astral_wire_buffer_free(&out);

    assert(astral_page_push(&page, &out));
    astral_wire_buffer_free(&out);
    assert(astral_page_push(&page, &out));
    astral_wire_buffer_free(&out);
    Batch navigate = {.length = 0};
    b8(&navigate, 1);
    children(&navigate, 5, 1, (int[]){6});
    assert(astral_settings_rewrite(navigate.bytes, navigate.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[4,[2046820608,"));
    assert(strstr(line, "[4,[2047082752,"));
    assert(astral_page_depth() == 0);
    astral_wire_buffer_free(&out);
    cardPageTest();
    categoryRowsTest();
    profileRelabelTest();
    gradientThemeTest();
    assert(astral_wire_rewrite_failures() == 0);
    nodeInsertTest();
    unsupportedVersionTest();
    puts("ok");
    return 0;
}
