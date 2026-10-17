#include "koeinput.h"
#include <fcitx-config/iniparser.h>
#include <dbus_public.h>
#include <notifications_public.h>
#include <fcitx-utils/capabilityflags.h>
#include <fcitx-utils/i18n.h>
#include <fcitx-utils/log.h>
#include <fcitx/addonfactory.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputpanel.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>

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
            if (e.inputContext() == pressIc_) pressIc_ = nullptr;
            if (e.inputContext() == sessionIc_.get()) {
                // The context is going away: drop it before the controller clears the display.
                sessionIc_.unwatch();
                controller_.focusLost();
            }
        }));
}

KoeInputAddon::~KoeInputAddon() = default;

void KoeInputAddon::reloadConfig() {
    fcitx::readAsIni(config_, kConfPath);
    applyConfig();
}

void KoeInputAddon::setConfig(const fcitx::RawConfig &config) {
    config_.load(config, true);
    fcitx::safeSaveAsIni(config_, kConfPath);
    applyConfig();
}

void KoeInputAddon::applyConfig() {
    controller_.setSettings(Settings{*config_.tapToggle,
                                     static_cast<uint64_t>(*config_.tapThreshold),
                                     *config_.showPartial});
}

bool KoeInputAddon::eligible(fcitx::InputContext *ic) const {
    if (!client_ || !ic || !ic->hasFocus()) return false;
    if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Password)) return false;
    const auto &panel = ic->inputPanel();
    if (!panel.preedit().empty() || !panel.clientPreedit().empty()) return false;
    const auto list = panel.candidateList();
    return !list || list->size() == 0;
}

void KoeInputAddon::handleKey(fcitx::KeyEvent &event) {
    const auto &key = event.key();
    bool swallow = false;
    if (key.checkKeyList(*config_.hotkey)) {
        if (event.isRelease()) {
            swallow = controller_.hotkeyRelease(nowMs());
        } else {
            pressIc_ = event.inputContext();
            swallow = controller_.hotkeyPress(nowMs(), eligible(pressIc_), key.isModifier());
        }
    } else if (!event.isRelease()) {
        swallow = key.sym() == FcitxKey_Escape ? controller_.escape() : controller_.otherKeyPress();
    }
    if (swallow) event.filterAndAccept();
}

void KoeInputAddon::callStart() {
    sessionIc_ = pressIc_ ? pressIc_->watch() : fcitx::TrackableObjectReference<fcitx::InputContext>();
    client_->start();
}

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
    if (!ic) return;
    if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Preedit)) {
        ic->inputPanel().setClientPreedit(fcitx::Text(text, fcitx::TextFormatFlag::Underline));
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
