#import "TomlDocument.h"

#include "AstralTOML.h"
#import "FoundationJSON.h"

NSDictionary *AstralParseTOML(NSData *data)
{
    if (!data.length) return nil;
    auto parsed = astral::parseTOML(std::string_view(static_cast<const char *>(data.bytes), data.length));
    return parsed.is_object() ? AstralObjectFromJSON(parsed) : nil;
}
