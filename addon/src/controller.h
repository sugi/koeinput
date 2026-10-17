#pragma once

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace koeinput {

enum class Status { Preparing, Recording, RecordingTap, Finalizing };

enum class Notice {
    NoDaemon,
    NoModel,
    Busy,
    NoResponse,
    AudioError,
    ModelError,
    InternalError,
    DaemonLeft,
    Failed,
};

enum class CallError { NoDaemon, NoModel, Busy, Timeout, Other };

CallError callErrorFromName(const std::string &dbusErrorName);

struct Settings {
    bool tapToggle = true;
    uint64_t tapThresholdMs = 300;
    bool showPartial = true;
};

// Implementations must not call back into the Controller from inside an action.
class Actions {
public:
    virtual ~Actions() = default;
    virtual void callStart() = 0;
    virtual void callStop(uint32_t id) = 0;
    virtual void callCancel(uint32_t id) = 0;
    virtual void showStatus(Status status) = 0;
    virtual void showPartial(const std::string &text) = 0;
    virtual void clearDisplay() = 0;
    virtual void commit(const std::string &text) = 0;
    virtual void notify(Notice notice, const std::string &detail) = 0;
    virtual void startTimer(uint64_t ms) = 0;
    virtual void stopTimer() = 0;
};

// Session logic of the addon: hotkey handling, the daemon's signals and
// what to display. It knows nothing about fcitx5 or D-Bus.
class Controller {
public:
    static constexpr uint64_t kRepeatGapMs = 1000;
    static constexpr uint64_t kFinalizeTimeoutMs = 10000;
    static constexpr std::size_t kMaxBuffered = 64;

    Controller(Actions &actions, Settings settings);

    void setSettings(const Settings &settings) { settings_ = settings; }

    // Key inputs return true when the key must be swallowed.
    bool hotkeyPress(uint64_t nowMs, bool eligible, bool modifier);
    bool hotkeyRelease(uint64_t nowMs);
    bool escape();
    bool otherKeyPress();
    void focusLost();

    void startReply(uint32_t id);
    void startFailed(CallError kind, const std::string &message);
    void stopFailed(uint32_t id, CallError kind, const std::string &message);
    void stateChanged(uint32_t id, const std::string &state);
    void partial(uint32_t id, const std::string &text);
    void finalText(uint32_t id, const std::string &text);
    void error(uint32_t id, const std::string &code, const std::string &message);
    void daemonLeft();
    void timerFired();

    bool idle() const { return phase_ == Phase::Idle; }

private:
    enum class Phase { Idle, Starting, Active, Finalizing };
    enum class SignalKind { State, Partial, Final, Error };
    struct Buffered {
        SignalKind kind;
        uint32_t id;
        std::string value;
        std::string detail;
    };

    void begin(uint64_t nowMs);
    void requestStop();
    void cancel();
    void finish();
    void startWatchdog();
    void showRecording();
    void releaseDisplay();
    bool current(uint32_t id) const { return id_ && *id_ == id; }
    void buffer(SignalKind kind, uint32_t id, const std::string &value, const std::string &detail = {});
    void notifyCallError(CallError kind, const std::string &message);

    Actions &actions_;
    Settings settings_;
    Phase phase_ = Phase::Idle;
    std::optional<uint32_t> id_;
    bool keyDown_ = false;
    bool ownsPress_ = false;
    bool startPress_ = false;
    uint64_t lastPressMs_ = 0;
    uint64_t pressStartMs_ = 0;
    bool tapMode_ = false;
    bool daemonRecording_ = false;
    bool pendingStop_ = false;
    bool pendingCancel_ = false;
    bool displayReleased_ = false;
    bool timerRunning_ = false;
    std::vector<Buffered> buffered_;
};

} // namespace koeinput
