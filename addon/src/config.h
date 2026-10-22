#pragma once

#include <fcitx-config/configuration.h>
#include <fcitx-config/enum.h>
#include <fcitx-config/option.h>
#include <fcitx-utils/i18n.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/macros.h>
#include <string>
#include <vector>

namespace koeinput {

FCITX_CONFIG_ENUM(PartialText, InApp, Panel, Off);

// The same shape as FCITX_CONFIG_ENUM_I18N_ANNOTATION, which insists that the
// UI names equal the stored names.
struct PartialTextI18NAnnotation {
    static constexpr const char *kNames[] = {N_("In the app"), N_("In the panel"), N_("Off")};
    static_assert(FCITX_ARRAY_SIZE(kNames) == FCITX_ARRAY_SIZE(_PartialText_Names));

    bool skipDescription() const { return false; }
    bool skipSave() const { return false; }
    void dumpDescription(fcitx::RawConfig &config) const {
        for (size_t i = 0; i < FCITX_ARRAY_SIZE(kNames); i++) {
            config.setValueByPath("EnumI18n/" + std::to_string(i), _(kNames[i]));
        }
    }
};

FCITX_CONFIGURATION(
    KoeInputConfig,
    fcitx::KeyListOption hotkey{
        this, "Hotkey", _("Hotkey"), {fcitx::Key(FcitxKey_Alt_R)},
        fcitx::KeyListConstrain({fcitx::KeyConstrainFlag::AllowModifierLess,
                                 fcitx::KeyConstrainFlag::AllowModifierOnly})};
    fcitx::Option<bool> tapToggle{this, "TapToggle", _("Tap to keep recording"), true};
    fcitx::Option<int, fcitx::IntConstrain> tapThreshold{
        this, "TapThreshold", _("Tap threshold (ms)"), 300, fcitx::IntConstrain(100, 1000)};
    fcitx::OptionWithAnnotation<PartialText, PartialTextI18NAnnotation> partialText{
        this, "PartialText", _("Partial text"), PartialText::InApp};
    fcitx::Option<std::vector<std::string>> panelApps{
        this, "PanelApps", _("Apps that show partial text in the panel"), {}};
    fcitx::Option<std::vector<std::string>> tailApps{
        this, "TailApps", _("Apps that show only the end of partial text"), {}};
    fcitx::Option<int, fcitx::IntConstrain> tailLength{
        this, "TailLength", _("Characters shown at the end"), 30, fcitx::IntConstrain(10, 1000)};);

} // namespace koeinput
