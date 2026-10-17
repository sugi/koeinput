#include "koeinput.h"
#include <fcitx-config/iniparser.h>
#include <fcitx-utils/i18n.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addonmanager.h>

namespace koeinput {

namespace {
constexpr char kConfPath[] = "conf/koeinput.conf";
}

KoeInputAddon::KoeInputAddon(fcitx::Instance *instance) : instance_(instance) {
    reloadConfig();
}

void KoeInputAddon::reloadConfig() { fcitx::readAsIni(config_, kConfPath); }

void KoeInputAddon::setConfig(const fcitx::RawConfig &config) {
    config_.load(config, true);
    fcitx::safeSaveAsIni(config_, kConfPath);
}

class KoeInputFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        fcitx::registerDomain(FCITX_GETTEXT_DOMAIN, FCITX_INSTALL_LOCALEDIR);
        return new KoeInputAddon(manager->instance());
    }
};

} // namespace koeinput

FCITX_ADDON_FACTORY(koeinput::KoeInputFactory);
