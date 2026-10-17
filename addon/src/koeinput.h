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
    enum class PartialTarget { Client, Panel, None };

    void handleKey(fcitx::KeyEvent &event);
    bool eligible(fcitx::InputContext *ic) const;
    PartialTarget partialTarget(const fcitx::InputContext &ic) const;
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
    // Chosen when the session starts, from the context's program and the settings.
    PartialTarget partialTarget_ = PartialTarget::None;
    // A pressed key, remembered by keycode and sym so its release pairs even
    // when the modifiers changed in between (Control+space released as space,
    // Alt_R released as Alt+Shift+Meta_R).
    struct PressedKey {
        int code = 0;
        fcitx::KeySym sym = FcitxKey_None;
        uint64_t pressMs = 0;
        // False once a later press lacked the key's modifier state: the key
        // is still remembered for its release, but it cannot start a chord.
        bool held = true;

        bool empty() const { return code == 0 && sym == FcitxKey_None; }
        bool matches(const fcitx::Key &key) const {
            return (code != 0 && key.code() == code) || (sym != FcitxKey_None && key.sym() == sym);
        }
    };

    bool hotkeyHeld(const fcitx::Key &rawKey, uint64_t nowMs) const;

    PressedKey hotkey_;
    // Other configured hotkeys pressed while hotkey_ is down; they do nothing.
    std::vector<PressedKey> chord_;
    bool escapeDown_ = false;
    std::unique_ptr<fcitx::EventSourceTime> timer_;
    std::vector<std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>>> watchers_;
    uint32_t notificationId_ = 0;
};

} // namespace koeinput
