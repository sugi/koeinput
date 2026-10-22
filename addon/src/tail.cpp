#include "tail.h"
#include <fcitx-utils/utf8.h>

namespace koeinput {

std::string tailText(const std::string &text, size_t limit) {
    if (limit == 0) return text;
    const size_t length = fcitx::utf8::length(text);
    if (length <= limit) return text;
    const auto start = fcitx::utf8::nextNChar(text.begin(), length - (limit - 1));
    return "\xE2\x80\xA6" + std::string(start, text.end());
}

} // namespace koeinput
