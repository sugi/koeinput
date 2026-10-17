#pragma once

#include "config.h"
#include "controller.h"
#include "daemonclient.h"
#include <fcitx-utils/event.h>
#include <fcitx-utils/handlertable.h>
#include <fcitx-utils/trackableobject.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputcontext.h>
#include <fcitx/instance.h>
#include <memory>
#include <vector>

namespace koeinput {

class KoeInputAddon : public fcitx::AddonInstance, private Actions {
public:
    explicit KoeInputAddon(fcitx::Instance *instance);
    ~KoeInputAddon() override;

    void reloadConfig() override;
    const fcitx::Configuration *getConfig() const override { return &config_; }
    void setConfig(const fcitx::RawConfig &config) override;

    FCITX_ADDON_DEPENDENCY_LOADER(dbus, instance_->addonManager());
    FCITX_ADDON_DEPENDENCY_LOADER(notifications, instance_->addonManager());

private:
    void handleKey(fcitx::KeyEvent &event);
    bool eligible(fcitx::InputContext *ic) const;
    void applyConfig();

    void callStart() override;
    void callStop(uint32_t id) override;
    void callCancel(uint32_t id) override;
    void showStatus(Status status) override;
    void showPartial(const std::string &text) override;
    void clearDisplay() override;
    void commit(const std::string &text) override;
    void notify(Notice notice, const std::string &detail) override;
    void startTimer(uint64_t ms) override;
    void stopTimer() override;

    fcitx::Instance *instance_;
    KoeInputConfig config_;
    Controller controller_;
    std::unique_ptr<DaemonClient> client_;
    fcitx::TrackableObjectReference<fcitx::InputContext> sessionIc_;
    // Keycode and sym of the last hotkey press, so its release pairs even when
    // the modifiers changed in between (Control+space released as space,
    // Alt_R released as Alt+Shift+Meta_R).
    int hotkeyCode_ = 0;
    fcitx::KeySym hotkeySym_ = FcitxKey_None;
    uint64_t hotkeyPressMs_ = 0;
    bool escapeDown_ = false;
    std::unique_ptr<fcitx::EventSourceTime> timer_;
    std::vector<std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>>> watchers_;
    uint32_t notificationId_ = 0;
};

} // namespace koeinput
