#include "koeinput.h"
#include <fcitx-config/iniparser.h>
#include <dbus_public.h>
#include <notifications_public.h>
#include <fcitx-utils/capabilityflags.h>
#include <fcitx-utils/i18n.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/stringutils.h>
#include <fcitx/addonfactory.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputpanel.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <algorithm>
#include <strings.h>

namespace koeinput {

namespace {

FCITX_DEFINE_LOG_CATEGORY(koeinput_log, "koeinput");
#define KOEINPUT_WARN() FCITX_LOGC(koeinput_log, Warn)

constexpr char kConfPath[] = "conf/koeinput.conf";

// 1 ms: sd-event turns accuracy 0 into its 250 ms default.
constexpr uint64_t kTimerAccuracyUsec = 1000;

std::string statusText(Status status) {
    switch (status) {
    case Status::Preparing: return _("Preparing…");
    case Status::Recording: return _("Recording");
    case Status::RecordingTap: return _("Recording (press again to finish)");
    case Status::Finalizing: return _("Recognizing…");
    }
    return {};
}

std::string withDetail(const char *text, const std::string &detail) {
    return detail.empty() ? std::string(text) : std::string(text) + "\n" + detail;
}

std::string noticeText(Notice notice, const std::string &detail) {
    switch (notice) {
    case Notice::NoDaemon: return _("The KoeInput daemon is not running.");
    case Notice::NoModel: return withDetail(_("The speech model is not ready."), detail);
    case Notice::Busy: return _("Another dictation is running.");
    case Notice::NoResponse: return _("The KoeInput daemon does not respond.");
    case Notice::AudioError: return withDetail(_("Cannot record from the microphone."), detail);
    case Notice::ModelError: return withDetail(_("Speech recognition failed."), detail);
    case Notice::InternalError: return withDetail(_("The KoeInput daemon reported an error."), detail);
    case Notice::DaemonLeft: return _("The KoeInput daemon stopped.");
    case Notice::Failed: return withDetail(_("Dictation failed."), detail);
    }
    return {};
}

uint64_t nowMs() { return fcitx::now(CLOCK_MONOTONIC) / 1000; }

// Key::isModifier() leaves out the level and group shifts (AltGr), which
// behave like modifiers: held with other keys and never auto-repeated.
bool isModifierKey(const fcitx::Key &key) {
    return key.isModifier() || key.sym() == FcitxKey_ISO_Level3_Shift ||
           key.sym() == FcitxKey_Mode_switch;
}

// The state a held modifier key adds to later key events. keySymToStates
// knows nothing about the level and group shifts, which XKB binds to Mod5.
fcitx::KeyStates modifierStates(fcitx::KeySym sym) {
    if (sym == FcitxKey_ISO_Level3_Shift || sym == FcitxKey_Mode_switch) return fcitx::KeyState::Mod5;
    return fcitx::Key::keySymToStates(sym);
}

} // namespace

KoeInputAddon::KoeInputAddon(fcitx::Instance *instance)
    : instance_(instance), controller_(*this, Settings{}) {
    reloadConfig();
    if (auto *dbusAddon = dbus()) {
        auto *bus = dbusAddon->call<fcitx::IDBusModule::bus>();
        DaemonCallbacks cb;
        cb.started = [this](uint32_t id) { controller_.startReply(id); };
        cb.startFailed = [this](CallError k, const std::string &m) { controller_.startFailed(k, m); };
        cb.stopFailed = [this](uint32_t id, CallError k, const std::string &m) {
            controller_.stopFailed(id, k, m);
        };
        cb.stateChanged = [this](uint32_t id, const std::string &s) { controller_.stateChanged(id, s); };
        cb.partial = [this](uint32_t id, const std::string &t) { controller_.partial(id, t); };
        cb.finalText = [this](uint32_t id, const std::string &t) { controller_.finalText(id, t); };
        cb.error = [this](uint32_t id, const std::string &c, const std::string &m) {
            controller_.error(id, c, m);
        };
        cb.daemonLeft = [this] { controller_.daemonLeft(); };
        cb.activateFailed = [](const std::string &m) {
            KOEINPUT_WARN() << "Cannot start the KoeInput daemon: " << m;
        };
        client_ = std::make_unique<DaemonClient>(*bus, std::move(cb));
    }
    watchers_.emplace_back(instance_->watchEvent(
        fcitx::EventType::InputContextKeyEvent, fcitx::EventWatcherPhase::PreInputMethod,
        [this](fcitx::Event &event) { handleKey(static_cast<fcitx::KeyEvent &>(event)); }));
    watchers_.emplace_back(instance_->watchEvent(
        fcitx::EventType::InputContextFocusOut, fcitx::EventWatcherPhase::Default,
        [this](fcitx::Event &event) {
            auto &e = static_cast<fcitx::InputContextEvent &>(event);
            if (e.inputContext() == sessionIc_.get()) controller_.focusLost();
        }));
    watchers_.emplace_back(instance_->watchEvent(
        fcitx::EventType::InputContextDestroyed, fcitx::EventWatcherPhase::Default,
        [this](fcitx::Event &event) {
            auto &e = static_cast<fcitx::InputContextEvent &>(event);
            if (e.inputContext() == sessionIc_.get()) {
                // The context is going away: drop it before the controller clears the display.
                sessionIc_.unwatch();
                controller_.focusLost();
            }
        }));
    // So the model is loaded by the first dictation.
    if (client_) client_->activate();
}

KoeInputAddon::~KoeInputAddon() = default;

void KoeInputAddon::reloadConfig() {
    fcitx::RawConfig raw;
    fcitx::readAsIni(raw, kConfPath);
    // Configs written before the "Partial text" setting have the boolean ShowPartial.
    if (!raw.get("PartialText")) {
        if (auto old = raw.get("ShowPartial"); old && old->value() == "False") {
            raw.setValueByPath("PartialText", PartialTextToString(PartialText::Off));
        }
    }
    config_.load(raw);
    applyConfig();
}

void KoeInputAddon::setConfig(const fcitx::RawConfig &config) {
    config_.load(config, true);
    fcitx::safeSaveAsIni(config_, kConfPath);
    applyConfig();
}

void KoeInputAddon::applyConfig() {
    // The controller only knows whether partial text can show at all; where it
    // goes is decided per session by partialTarget().
    const bool anyPartial = *config_.partialText != PartialText::Off || !config_.panelApps->empty();
    controller_.setSettings(Settings{*config_.tapToggle,
                                     static_cast<uint64_t>(*config_.tapThreshold), anyPartial});
}

KoeInputAddon::PartialTarget KoeInputAddon::partialTarget(const fcitx::InputContext &ic) const {
    const auto &apps = *config_.panelApps;
    const bool listed = std::any_of(apps.begin(), apps.end(), [&](const std::string &entry) {
        const std::string app = fcitx::stringutils::trim(entry);
        return !app.empty() && strcasecmp(app.c_str(), ic.program().c_str()) == 0;
    });
    if (listed) return PartialTarget::Panel;
    switch (*config_.partialText) {
    case PartialText::Off: return PartialTarget::None;
    case PartialText::Panel: return PartialTarget::Panel;
    case PartialText::InApp:
        return ic.capabilityFlags().test(fcitx::CapabilityFlag::Preedit) ? PartialTarget::Client
                                                                          : PartialTarget::Panel;
    }
    return PartialTarget::None;
}

bool KoeInputAddon::eligible(fcitx::InputContext *ic) const {
    if (!client_ || !ic || !ic->hasFocus()) return false;
    if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Password)) return false;
    const auto &panel = ic->inputPanel();
    if (!panel.preedit().empty() || !panel.clientPreedit().empty()) return false;
    const auto list = panel.candidateList();
    return !list || list->size() == 0;
}

// The hotkey's release may have been lost (some X11 paths drop releases), so
// a chord needs evidence that the key is still down: a held modifier shows in
// the states of later keys; a plain key auto-repeats. The raw key keeps every
// state bit; the normalized one may not.
bool KoeInputAddon::hotkeyHeld(const PressedKey &hotkey, const fcitx::Key &rawKey, uint64_t nowMs) {
    if (hotkey.empty() || !hotkey.held) return false;
    const fcitx::KeyStates modifier = modifierStates(hotkey.sym);
    if (modifier) return (rawKey.states() & modifier) == modifier;
    return nowMs - hotkey.pressMs <= Controller::kRepeatGapMs;
}

// Frontends report the state before the press, so a held modifier hotkey
// shows in every later press. A press without it means the release was
// lost; the key stays stored so its release still pairs, but cannot start
// a chord.
void KoeInputAddon::dropHeldEvidence(PressedKey &hotkey, const fcitx::Key &rawKey) {
    if (!hotkey.held) return;
    const fcitx::KeyStates modifier = modifierStates(hotkey.sym);
    if (modifier && (rawKey.states() & modifier) != modifier) hotkey.held = false;
}

bool KoeInputAddon::hotkeyPress(fcitx::KeyEvent &event, uint64_t nowMs, bool flagged) {
    auto *ic = event.inputContext();
    // The controller shows the first status before it calls callStart().
    if (controller_.idle()) {
        sessionIc_ = ic->watch();
        partialTarget_ = partialTarget(*ic);
    }
    return controller_.hotkeyPress(nowMs, eligible(ic), isModifierKey(event.key()), flagged);
}

void KoeInputAddon::handleKey(fcitx::KeyEvent &event) {
    // key() is normalized: it has lost the Repeat state and may have lost
    // Mod5; rawKey() keeps them and the keycode.
    if (const int code = event.rawKey().code(); code != 0) {
        handleKeyByCode(event, code);
    } else {
        handleKeyBySym(event);
    }
}

// Every press is recorded under its keycode and its release goes the same
// way: swallowed if the press was, passed if the press passed. The sym and
// the states of the release do not matter (Control+space is released as
// space, Alt_R as Alt+Shift+Meta_R), and a release without a press is not
// ours. A new press of a recorded keycode is its auto-repeat, or its release
// was lost: the record is replaced.
void KoeInputAddon::handleKeyByCode(fcitx::KeyEvent &event, int code) {
    const auto &key = event.key();
    const uint64_t now = nowMs();
    if (event.isRelease()) {
        const auto it = pressed_.find(code);
        if (it == pressed_.end()) return;
        const KeyRecord record = it->second;
        pressed_.erase(it);
        if (record.role == Role::Hotkey) {
            hotkeyCode_ = 0;
            controller_.hotkeyRelease(now);
        }
        if (record.swallowed) event.filterAndAccept();
        return;
    }
    const bool flagged = event.rawKey().states().test(fcitx::KeyState::Repeat);
    const auto hotkeyIt = pressed_.find(hotkeyCode_);
    KeyRecord *hotkey = hotkeyIt == pressed_.end() ? nullptr : &hotkeyIt->second;
    if (hotkey && code != hotkeyCode_) dropHeldEvidence(*hotkey, event.rawKey());
    if (const auto it = pressed_.find(code); it != pressed_.end()) {
        KeyRecord &record = it->second;
        // The auto-repeat of a held key can arrive under another sym once its
        // modifiers went up (Control+space repeating as space). Modifier keys
        // never auto-repeat, and after a longer gap the release was lost: then
        // the press is a new key.
        const bool repeat = flagged || (!isModifierKey(fcitx::Key(record.sym)) &&
                                        now - record.pressMs <= Controller::kRepeatGapMs);
        if (repeat && record.role == Role::Hotkey) {
            record.pressMs = now;
            if (controller_.hotkeyPress(now, eligible(event.inputContext()), isModifierKey(key), true)) {
                event.filterAndAccept();
            }
            return;
        }
        if (repeat && record.role == Role::Chord) {
            record.pressMs = now;
            event.filterAndAccept();
            return;
        }
    }
    KeyRecord record{{key.sym(), now}};
    if (key.checkKeyList(*config_.hotkey)) {
        if (hotkey && code != hotkeyCode_ && hotkeyHeld(*hotkey, event.rawKey(), now)) {
            // Another configured hotkey while the first is still down is a chord.
            record.role = Role::Chord;
            record.swallowed = true;
        } else {
            // The controller follows one press at a time. A hotkey press whose
            // release never came keeps its record, so the release is still
            // swallowed when it arrives, but it no longer reaches the controller.
            if (hotkey && code != hotkeyCode_) hotkey->role = Role::Other;
            record.role = Role::Hotkey;
            record.swallowed = hotkeyPress(event, now, flagged);
            hotkeyCode_ = code;
        }
    } else if (key.sym() == FcitxKey_Escape) {
        record.role = Role::Escape;
        record.swallowed = controller_.escape();
    } else if (!isModifierKey(key)) {
        // A modifier alone is not "another key": it may be the start of a
        // combination hotkey that stops a tap-mode session.
        controller_.otherKeyPress();
    }
    // Another sym on the hotkey's keycode (Shift+Meta_R on the Alt_R key)
    // means the hotkey's release was lost.
    if (code == hotkeyCode_ && record.role != Role::Hotkey) hotkeyCode_ = 0;
    pressed_[code] = record;
    if (record.swallowed) event.filterAndAccept();
}

// Without keycodes a release pairs by sym: the stored hotkey's sym, a
// modifier release of a configured modifier hotkey, or the configured
// hotkey itself. These rules cannot tell a key apart from the hotkey on
// the same physical key once the modifiers changed.
void KoeInputAddon::handleKeyBySym(fcitx::KeyEvent &event) {
    const auto &key = event.key();
    const uint64_t now = nowMs();
    const bool sameKey = hotkey_.matches(key);
    const auto chordKey = std::find_if(chord_.begin(), chord_.end(),
                                       [&](const PressedKey &k) { return k.matches(key); });
    bool swallow = false;
    if (event.isRelease()) {
        if (chordKey != chord_.end()) {
            chord_.erase(chordKey);
            event.filterAndAccept();
            return;
        }
        const auto &hotkeys = *config_.hotkey;
        const bool modifierRelease =
            std::any_of(hotkeys.begin(), hotkeys.end(), [&](const fcitx::Key &k) {
                return k.isModifier() && key.isReleaseOfModifier(k);
            });
        if (sameKey || modifierRelease || key.checkKeyList(hotkeys)) {
            hotkey_ = {};
            swallow = controller_.hotkeyRelease(now);
        } else if (key.sym() == FcitxKey_Escape && escapeDown_) {
            escapeDown_ = false;
            swallow = true;
        }
        if (swallow) event.filterAndAccept();
        return;
    }
    const bool flagged = event.rawKey().states().test(fcitx::KeyState::Repeat);
    if (!sameKey) dropHeldEvidence(hotkey_, event.rawKey());
    if (chordKey != chord_.end()) {
        // A chord key repeats like the hotkey below (Alt_R released first, the
        // chord's space still down); after a longer gap its release was lost.
        if (flagged || now - chordKey->pressMs <= Controller::kRepeatGapMs) {
            chordKey->pressMs = now;
            event.filterAndAccept();
            return;
        }
        chord_.erase(chordKey);
    }
    // Once the modifiers of a combination hotkey are released, its auto-repeat
    // arrives as the bare key. Frontends that flag repeats settle it; without
    // the flag, a bare press after a longer gap means the key was released and
    // the release lost, so the press is an ordinary key again.
    const bool repeat = sameKey && (flagged || now - hotkey_.pressMs <= Controller::kRepeatGapMs);
    if (repeat || key.checkKeyList(*config_.hotkey)) {
        // Another configured hotkey while the first is still down is a chord.
        if (!sameKey && hotkeyHeld(hotkey_, event.rawKey(), now)) {
            chord_.push_back({key.sym(), now});
            event.filterAndAccept();
            return;
        }
        hotkey_ = {key.sym(), now};
        swallow = hotkeyPress(event, now, flagged);
    } else {
        // The hotkey's sym again after the repeat gap means its release was
        // lost: let this key's release pass.
        if (sameKey) hotkey_ = {};
        if (key.sym() == FcitxKey_Escape) {
            swallow = controller_.escape();
            escapeDown_ = swallow;
        } else if (!isModifierKey(key)) {
            // A modifier alone is not "another key": it may be the start of a
            // combination hotkey that stops a tap-mode session.
            swallow = controller_.otherKeyPress();
        }
    }
    if (swallow) event.filterAndAccept();
}

void KoeInputAddon::callStart() { client_->start(); }

void KoeInputAddon::callStop(uint32_t id) { client_->stop(id); }

void KoeInputAddon::callCancel(uint32_t id) { client_->cancel(id); }

void KoeInputAddon::showStatus(Status status) {
    auto *ic = sessionIc_.get();
    if (!ic) return;
    ic->inputPanel().setAuxUp(fcitx::Text(statusText(status)));
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void KoeInputAddon::showPartial(const std::string &text) {
    auto *ic = sessionIc_.get();
    if (!ic || partialTarget_ == PartialTarget::None) return;
    if (partialTarget_ == PartialTarget::Client) {
        // DontCommit: fcitx5 would otherwise commit the client preedit on focus out.
        fcitx::Text preedit(text, fcitx::TextFormatFlags(
                                      {fcitx::TextFormatFlag::Underline, fcitx::TextFormatFlag::DontCommit}));
        preedit.setCursor(static_cast<int>(text.size()));
        ic->inputPanel().setClientPreedit(preedit);
        ic->updatePreedit();
    } else {
        ic->inputPanel().setPreedit(fcitx::Text(text));
    }
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void KoeInputAddon::clearDisplay() {
    auto *ic = sessionIc_.get();
    if (!ic) return;
    auto &panel = ic->inputPanel();
    panel.setAuxUp(fcitx::Text());
    panel.setPreedit(fcitx::Text());
    panel.setClientPreedit(fcitx::Text());
    ic->updatePreedit();
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void KoeInputAddon::commit(const std::string &text) {
    if (auto *ic = sessionIc_.get()) ic->commitString(text);
}

void KoeInputAddon::notify(Notice notice, const std::string &detail) {
    const std::string body = noticeText(notice, detail);
    KOEINPUT_WARN() << body;
    auto *n = notifications();
    if (!n) return;
    notificationId_ = n->call<fcitx::INotifications::sendNotification>(
        "fcitx5-koeinput", notificationId_, "audio-input-microphone", _("KoeInput"), body,
        std::vector<std::string>{}, 5000, nullptr, nullptr);
}

void KoeInputAddon::startTimer(uint64_t ms) {
    timer_ = instance_->eventLoop().addTimeEvent(
        CLOCK_MONOTONIC, fcitx::now(CLOCK_MONOTONIC) + ms * 1000, kTimerAccuracyUsec,
        [this](fcitx::EventSourceTime *, uint64_t) {
            controller_.timerFired();
            return true;
        });
}

void KoeInputAddon::stopTimer() { timer_.reset(); }

class KoeInputFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        fcitx::registerDomain(FCITX_GETTEXT_DOMAIN, FCITX_INSTALL_LOCALEDIR);
        return new KoeInputAddon(manager->instance());
    }
};

} // namespace koeinput

FCITX_ADDON_FACTORY(koeinput::KoeInputFactory);
