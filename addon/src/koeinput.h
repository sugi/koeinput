#pragma once

#include "config.h"
#include <fcitx/addoninstance.h>
#include <fcitx/instance.h>

namespace koeinput {

class KoeInputAddon : public fcitx::AddonInstance {
public:
    explicit KoeInputAddon(fcitx::Instance *instance);

    void reloadConfig() override;
    const fcitx::Configuration *getConfig() const override { return &config_; }
    void setConfig(const fcitx::RawConfig &config) override;

private:
    fcitx::Instance *instance_;
    KoeInputConfig config_;
};

} // namespace koeinput
