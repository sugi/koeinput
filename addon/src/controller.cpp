#include "controller.h"
#include <algorithm>
#include <utility>

namespace koeinput {

CallError callErrorFromName(const std::string &name) {
    if (name == "org.freedesktop.DBus.Error.ServiceUnknown" ||
        name == "org.freedesktop.DBus.Error.NameHasNoOwner" ||
        name.starts_with("org.freedesktop.DBus.Error.Spawn.")) {
        return CallError::NoDaemon;
    }
    if (name == "org.nemui.KoeInput.Error.NoModel") return CallError::NoModel;
    if (name == "org.nemui.KoeInput.Error.Busy") return CallError::Busy;
    if (name == "org.freedesktop.DBus.Error.NoReply" ||
        name == "org.freedesktop.DBus.Error.Timeout" ||
        name == "org.freedesktop.DBus.Error.TimedOut") {
        return CallError::Timeout;
    }
    return CallError::Other;
}

Controller::Controller(Actions &actions, Settings settings)
    : actions_(actions), settings_(settings) {}

bool Controller::hotkeyPress(uint64_t nowMs, bool eligible, bool modifier) {
    // Modifier keys never auto-repeat, so a press while one is down means the
    // release was lost. A plain key first repeats after the desktop's initial
    // delay of 500 to 660 ms, so the gap must be longer than that.
    const bool repeat = !modifier && keyDown_ && nowMs - lastPressMs_ <= kRepeatGapMs;
    lastPressMs_ = nowMs;
    if (repeat) return ownsPress_;
    keyDown_ = true;
    startPress_ = false;
    switch (phase_) {
    case Phase::Idle:
        ownsPress_ = eligible;
        if (eligible) {
            startPress_ = true;
            begin(nowMs);
        }
        return ownsPress_;
    case Phase::Starting:
    case Phase::Active:
        ownsPress_ = true;
        requestStop();
        return true;
    case Phase::Finalizing:
        ownsPress_ = true;
        return true;
    }
    return false;
}

bool Controller::hotkeyRelease(uint64_t nowMs) {
    if (!keyDown_) return false;
    keyDown_ = false;
    const bool owned = ownsPress_;
    const bool startPress = startPress_;
    ownsPress_ = false;
    startPress_ = false;
    if (!owned) return false;
    const bool running = (phase_ == Phase::Starting && !pendingStop_ && !pendingCancel_) ||
                         phase_ == Phase::Active;
    if (startPress && running && !tapMode_) {
        if (settings_.tapToggle && nowMs - pressStartMs_ < settings_.tapThresholdMs) {
            tapMode_ = true;
            if (phase_ == Phase::Active && daemonRecording_) showRecording();
        } else {
            requestStop();
        }
    }
    return true;
}

bool Controller::escape() {
    if (phase_ == Phase::Idle) return false;
    cancel();
    return true;
}

bool Controller::otherKeyPress() {
    const bool userStopped =
        phase_ == Phase::Finalizing || (phase_ == Phase::Starting && pendingStop_);
    if (userStopped) {
        releaseDisplay();
    } else if (phase_ != Phase::Idle) {
        cancel();
    }
    return false;
}

void Controller::focusLost() { cancel(); }

void Controller::startReply(uint32_t id) {
    if (phase_ != Phase::Starting) {
        cancelStray(id);
        return;
    }
    stopWatchdog();
    if (pendingCancel_) {
        cancelSession(id);
        finish();
        return;
    }
    phase_ = Phase::Active;
    id_ = id;
    auto pending = std::move(buffered_);
    buffered_.clear();
    for (const auto &b : pending) {
        if (phase_ == Phase::Idle) break;
        switch (b.kind) {
        case SignalKind::State: stateChanged(b.id, b.value); break;
        case SignalKind::Partial: partial(b.id, b.value); break;
        case SignalKind::Final: finalText(b.id, b.value); break;
        case SignalKind::Error: error(b.id, b.value, b.detail); break;
        }
    }
    if (phase_ == Phase::Active && pendingStop_) {
        pendingStop_ = false;
        requestStop();
    }
}

void Controller::startFailed(CallError kind, const std::string &message) {
    if (phase_ != Phase::Starting) return;
    const bool quiet = pendingCancel_;
    finish();
    if (!quiet) notifyCallError(kind, message);
}

void Controller::stopFailed(uint32_t id, CallError kind, const std::string &message) {
    if (phase_ != Phase::Finalizing || !current(id)) return;
    finish();
    notifyCallError(kind, message);
}

void Controller::stateChanged(uint32_t id, const std::string &state) {
    if (phase_ == Phase::Starting) {
        buffer(SignalKind::State, id, state);
        return;
    }
    if (!current(id)) {
        if (state == "starting" || state == "recording") cancelStray(id);
        return;
    }
    if (state == "recording") {
        daemonRecording_ = true;
        if (phase_ == Phase::Active) showRecording();
    } else if (state == "finalizing") {
        if (phase_ == Phase::Active) {
            phase_ = Phase::Finalizing;
            if (!displayReleased_) actions_.showStatus(Status::Finalizing);
            startWatchdog(kFinalizeTimeoutMs);
        }
    } else if (state == "done" || state == "cancelled" || state == "error") {
        finish();
    }
}

void Controller::partial(uint32_t id, const std::string &text) {
    if (phase_ == Phase::Starting) {
        buffer(SignalKind::Partial, id, text);
        return;
    }
    if (!current(id)) {
        cancelStray(id);
        return;
    }
    if (!settings_.showPartial || displayReleased_) return;
    actions_.showPartial(text);
}

void Controller::finalText(uint32_t id, const std::string &text) {
    if (phase_ == Phase::Starting) {
        buffer(SignalKind::Final, id, text);
        return;
    }
    if (!current(id)) return;
    finish();
    if (!text.empty()) actions_.commit(text);
}

void Controller::error(uint32_t id, const std::string &code, const std::string &message) {
    if (phase_ == Phase::Starting) {
        buffer(SignalKind::Error, id, code, message);
        return;
    }
    if (!current(id)) return;
    finish();
    const Notice n = code == "audio"   ? Notice::AudioError
                     : code == "model" ? Notice::ModelError
                                       : Notice::InternalError;
    actions_.notify(n, message);
}

void Controller::daemonLeft() {
    if (phase_ == Phase::Idle) return;
    const bool quiet = pendingCancel_;
    finish();
    if (!quiet) actions_.notify(Notice::DaemonLeft, "");
}

void Controller::timerFired() {
    timerRunning_ = false;
    if (phase_ == Phase::Starting) {
        const bool quiet = pendingCancel_;
        finish();
        if (!quiet) actions_.notify(Notice::NoResponse, "");
        return;
    }
    if (phase_ != Phase::Finalizing) return;
    cancelSession(*id_);
    finish();
    actions_.notify(Notice::NoResponse, "");
}

void Controller::begin(uint64_t nowMs) {
    phase_ = Phase::Starting;
    id_.reset();
    pressStartMs_ = nowMs;
    tapMode_ = false;
    daemonRecording_ = false;
    pendingStop_ = false;
    pendingCancel_ = false;
    displayReleased_ = false;
    buffered_.clear();
    actions_.showStatus(Status::Preparing);
    startWatchdog(kStartTimeoutMs);
    actions_.callStart();
}

void Controller::requestStop() {
    if (phase_ == Phase::Starting) {
        if (!pendingCancel_) pendingStop_ = true;
        return;
    }
    if (phase_ != Phase::Active) return;
    phase_ = Phase::Finalizing;
    if (!displayReleased_) actions_.showStatus(Status::Finalizing);
    startWatchdog(kFinalizeTimeoutMs);
    actions_.callStop(*id_);
}

void Controller::cancel() {
    switch (phase_) {
    case Phase::Idle:
        return;
    case Phase::Starting:
        if (!pendingCancel_) {
            pendingCancel_ = true;
            pendingStop_ = false;
            releaseDisplay();
        }
        return;
    case Phase::Active:
    case Phase::Finalizing:
        cancelSession(*id_);
        finish();
        return;
    }
}

void Controller::finish() {
    stopWatchdog();
    if (!displayReleased_) actions_.clearDisplay();
    phase_ = Phase::Idle;
    id_.reset();
    tapMode_ = false;
    daemonRecording_ = false;
    pendingStop_ = false;
    pendingCancel_ = false;
    displayReleased_ = false;
    buffered_.clear();
}

void Controller::startWatchdog(uint64_t ms) {
    actions_.startTimer(ms);
    timerRunning_ = true;
}

void Controller::stopWatchdog() {
    if (!timerRunning_) return;
    actions_.stopTimer();
    timerRunning_ = false;
}

void Controller::cancelSession(uint32_t id) {
    lastCancelledId_ = id;
    actions_.callCancel(id);
}

// The daemon sends session signals only to the client that started the
// session, so an unknown id is a session this addon lost track of.
void Controller::cancelStray(uint32_t id) {
    if (lastCancelledId_ == id) return;
    cancelSession(id);
}

void Controller::showRecording() {
    if (!displayReleased_) actions_.showStatus(tapMode_ ? Status::RecordingTap : Status::Recording);
}

void Controller::releaseDisplay() {
    if (displayReleased_) return;
    actions_.clearDisplay();
    displayReleased_ = true;
}

void Controller::buffer(SignalKind kind, uint32_t id, const std::string &value, const std::string &detail) {
    if (buffered_.size() < kMaxBuffered) {
        buffered_.push_back({kind, id, value, detail});
        return;
    }
    if (kind == SignalKind::Partial) return;
    auto partial = std::find_if(buffered_.begin(), buffered_.end(),
                                [](const Buffered &b) { return b.kind == SignalKind::Partial; });
    if (partial == buffered_.end()) return;
    buffered_.erase(partial);
    buffered_.push_back({kind, id, value, detail});
}

void Controller::notifyCallError(CallError kind, const std::string &message) {
    switch (kind) {
    case CallError::NoDaemon: actions_.notify(Notice::NoDaemon, ""); return;
    case CallError::NoModel: actions_.notify(Notice::NoModel, message); return;
    case CallError::Busy: actions_.notify(Notice::Busy, ""); return;
    case CallError::Timeout: actions_.notify(Notice::NoResponse, ""); return;
    case CallError::Other: actions_.notify(Notice::Failed, message); return;
    }
}

} // namespace koeinput
