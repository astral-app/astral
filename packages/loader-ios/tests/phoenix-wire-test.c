#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "support/wire-parser.h"
#include "../sources/PhoenixWire.c"

typedef struct {
    unsigned char bytes[8192];
    size_t length;
} Batch;

static void b8(Batch *b, unsigned v)
{
    b->bytes[b->length++] = (unsigned char)v;
}
static void b16(Batch *b, unsigned v)
{
    b8(b, v & 255);
    b8(b, v >> 8);
}
static void b32(Batch *b, int v)
{
    memcpy(b->bytes + b->length, &v, 4);
    b->length += 4;
}
static void bstr(Batch *b, const char *s)
{
    b32(b, (int)strlen(s));
    memcpy(b->bytes + b->length, s, strlen(s));
    b->length += strlen(s);
}

static void create(Batch *b, int id, const char *type, int props)
{
    b8(b, 1);
    b32(b, id);
    b16(b, (unsigned)strlen(type));
    memcpy(b->bytes + b->length, type, strlen(type));
    b->length += strlen(type);
    b16(b, (unsigned)props);
}

static void children(Batch *b, int parent, int count, const int *ids)
{
    b8(b, 3);
    b32(b, parent);
    b32(b, count);
    for (int i = 0; i < count; i++) b32(b, ids[i]);
}

static char line[1 << 20];
static void sink(void *c, const char *l, size_t n)
{
    (void)c;
    memcpy(line, l, n);
    line[n] = 0;
}

static void skeleton(Batch *b)
{
    create(b, 5, "navigator", 0);
    create(b, 50, "navigator", 0);
    create(b, 51, "view", 0);
    create(b, 6, "view", 0);
    const int plain[] = {100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 112, 113, 115, 116};
    for (size_t i = 0; i < sizeof(plain) / sizeof(*plain); i++) create(b, plain[i], "view", 0);
    create(b, 111, "view", 2);
    b16(b, 9);
    b8(b, 6);
    bstr(b, "Back");
    b16(b, 740);
    b8(b, 12);
    b32(b, 0);
    b16(b, 0);
    create(b, 114, "image", 1);
    b16(b, 542);
    b8(b, 6);
    bstr(b, "res://ic_mana_x_small");
    create(b, 117, "text", 1);
    b16(b, 499);
    b8(b, 6);
    bstr(b, "Settings");
    create(b, 30, "view", 0);
    create(b, 31, "view", 0);
}

static void links(Batch *b)
{
    children(b, 30, 1, (int[]){31});
    children(b, 100, 1, (int[]){101});
    children(b, 101, 1, (int[]){102});
    children(b, 102, 1, (int[]){103});
    children(b, 103, 4, (int[]){104, 105, 106, 107});
    children(b, 106, 1, (int[]){10});
    children(b, 107, 2, (int[]){108, 115});
    children(b, 108, 1, (int[]){109});
    children(b, 109, 1, (int[]){110});
    children(b, 110, 1, (int[]){111});
    children(b, 111, 1, (int[]){112});
    children(b, 112, 1, (int[]){113});
    children(b, 113, 1, (int[]){114});
    children(b, 115, 1, (int[]){116});
    children(b, 116, 1, (int[]){117});
    children(b, 5, 2, (int[]){6, 100});
    children(b, 50, 1, (int[]){51});
    b8(b, 2);
    b32(b, 5);
    b16(b, 1);
    b16(b, 252);
    b8(b, 3);
    b32(b, 1);
}

static void overview(Batch *b)
{
    b->length = 0;
    b8(b, 1);
    skeleton(b);
    create(b, 10, "view", 0);
    create(b, 11, "input", 0);
    create(b, 20, "view", 1);
    b16(b, 147);
    b8(b, 6);
    bstr(b, "column");
    create(b, 21, "text", 1);
    b16(b, 499);
    b8(b, 6);
    bstr(b, "Account Settings");
    create(b, 22, "view", 0);
    create(b, 23, "view", 4);
    b16(b, 9);
    b8(b, 6);
    bstr(b, "settings.row.account");
    b16(b, 11);
    b8(b, 6);
    bstr(b, "Account");
    b16(b, 320);
    b8(b, 7);
    b32(b, 1);
    b8(b, 6);
    bstr(b, "press");
    b16(b, 733);
    b8(b, 11);
    b32(b, 2);
    b32(b, 7);
    b32(b, 9);
    create(b, 24, "view", 0);
    create(b, 25, "image", 1);
    b16(b, 542);
    b8(b, 6);
    bstr(b, "res://ic_mana_user_circle");
    create(b, 26, "text", 1);
    b16(b, 499);
    b8(b, 6);
    bstr(b, "Account");
    create(b, 27, "view", 0);
    create(b, 28, "image", 0);
    children(b, 27, 1, (int[]){28});
    children(b, 24, 3, (int[]){25, 26, 27});
    children(b, 23, 1, (int[]){24});
    children(b, 22, 2, (int[]){23, 30});
    children(b, 20, 2, (int[]){21, 22});
    children(b, 10, 2, (int[]){11, 20});
    links(b);
}

static size_t tagOwner(const AstralDecorationContext *context, AstralTag *tags, size_t capacity)
{
    if (capacity < 1 || context->user_id != 100000000000000001ull || context->channel_id != 100000000000000002ull) return 0;
    strcpy(tags[0].text, "OWNER");
    tags[0].background = 0xFFED4245;
    tags[0].foreground = 0xFFFFFFFF;
    return 1;
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
    astral_wire_buffer_free(&out);

    Batch again = {.length = 0};
    b8(&again, 1);
    children(&again, 920, 2, (int[]){921, 922});
    assert(astral_settings_rewrite(again.bytes, again.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[4,[2063597568,2063597569]]"));
    assert(strstr(line, "[3,920,[921,922,2063597570]]"));
    astral_wire_buffer_free(&out);

    Batch gone = {.length = 0};
    b8(&gone, 1);
    b8(&gone, 4);
    b32(&gone, 1);
    b32(&gone, 920);
    assert(astral_settings_rewrite(gone.bytes, gone.length, &out));
    astral_wire_summarise(out.bytes, out.length, sink, NULL);
    assert(strstr(line, "[4,[2063597570,2063597571]]"));
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
    put8(&out, 1);
    buildTag(&out, &tag, &slot);
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

int main(void)
{
    decorationTest();
    gradientTagTest();
    relabelTest();
    paletteTest();
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
    puts("ok");
    return 0;
}
