#pragma once

#include <fcitx-config/configuration.h>
#include <fcitx-config/option.h>
#include <fcitx-utils/i18n.h>
#include <fcitx-utils/key.h>

namespace koeinput {

FCITX_CONFIGURATION(
    KoeInputConfig,
    fcitx::KeyListOption hotkey{
        this, "Hotkey", _("Hotkey"), {fcitx::Key(FcitxKey_Alt_R)},
        fcitx::KeyListConstrain({fcitx::KeyConstrainFlag::AllowModifierLess,
                                 fcitx::KeyConstrainFlag::AllowModifierOnly})};
    fcitx::Option<bool> tapToggle{this, "TapToggle", _("Tap to keep recording"), true};
    fcitx::Option<int, fcitx::IntConstrain> tapThreshold{
        this, "TapThreshold", _("Tap threshold (ms)"), 300, fcitx::IntConstrain(100, 1000)};
    fcitx::Option<bool> showPartial{this, "ShowPartial", _("Show partial results"), true};);

} // namespace koeinput
