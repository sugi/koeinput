#pragma once

#include <cstddef>
#include <string>

namespace koeinput {

// Returns text unchanged when it has at most limit code points (or limit is 0);
// otherwise an ellipsis followed by the last limit - 1 code points.
std::string tailText(const std::string &text, size_t limit);

} // namespace koeinput
