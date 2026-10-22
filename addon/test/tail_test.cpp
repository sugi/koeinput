#include "../src/tail.h"
#include <cstdio>
#include <fcitx-utils/utf8.h>
#include <string>

using namespace koeinput;

namespace {

int failures = 0;

void checkText(const char *file, int line, const std::string &actual, const std::string &expected) {
    if (actual == expected) return;
    ++failures;
    std::fprintf(stderr, "%s:%d: actual [%s], expected [%s]\n", file, line, actual.c_str(),
                 expected.c_str());
}
#define CHECK_TEXT(actual, expected) checkText(__FILE__, __LINE__, (actual), (expected))

} // namespace

int main() {
    CHECK_TEXT(tailText("abcdef", 0), "abcdef");
    CHECK_TEXT(tailText("", 10), "");
    CHECK_TEXT(tailText("abc", 10), "abc");
    CHECK_TEXT(tailText("abcdefghij", 10), "abcdefghij");
    CHECK_TEXT(tailText("abcdefghijk", 10), "\xE2\x80\xA6" "cdefghijk");
    CHECK_TEXT(tailText("あいうえおかきくけこ", 10), "あいうえおかきくけこ");
    const std::string japanese = tailText("あいうえおかきくけこさしすせそ", 10);
    CHECK_TEXT(japanese, "\xE2\x80\xA6" "きくけこさしすせそ");
    if (!fcitx::utf8::validate(japanese) || fcitx::utf8::length(japanese) != 10) {
        ++failures;
        std::fprintf(stderr, "%s:%d: not 10 valid code points\n", __FILE__, __LINE__);
    }
    CHECK_TEXT(tailText("abあいうえおcdefg", 10), "\xE2\x80\xA6" "いうえおcdefg");
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
