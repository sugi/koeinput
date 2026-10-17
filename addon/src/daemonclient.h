#pragma once

#include "controller.h"
#include <cstdint>
#include <fcitx-utils/dbus/bus.h>
#include <fcitx-utils/dbus/servicewatcher.h>
#include <functional>
#include <memory>
#include <string>

namespace koeinput {

inline constexpr char kBusName[] = "org.nemui.KoeInput";
inline constexpr char kObjectPath[] = "/org/nemui/KoeInput";
inline constexpr char kInterface[] = "org.nemui.KoeInput.Recognizer1";

struct DaemonCallbacks {
    std::function<void(uint32_t id)> started;
    std::function<void(CallError kind, const std::string &message)> startFailed;
    std::function<void(uint32_t id, CallError kind, const std::string &message)> stopFailed;
    std::function<void(uint32_t id, const std::string &state)> stateChanged;
    std::function<void(uint32_t id, const std::string &text)> partial;
    std::function<void(uint32_t id, const std::string &text)> finalText;
    std::function<void(uint32_t id, const std::string &code, const std::string &message)> error;
    std::function<void()> daemonLeft;
};

// Asynchronous client of the KoeInput daemon on an fcitx5 D-Bus connection.
// Destroying it drops every pending reply and subscription.
// A callback must not call the same method of this client again synchronously:
// the new call replaces that method's slot, which is the one running.
class DaemonClient {
public:
    static constexpr uint64_t kStartTimeoutUsec = 5000000;
    static constexpr uint64_t kCallTimeoutUsec = 3000000;

    DaemonClient(fcitx::dbus::Bus &bus, DaemonCallbacks callbacks);
    ~DaemonClient();

    void start();
    void stop(uint32_t id);
    void cancel(uint32_t id);

private:
    bool onSignal(fcitx::dbus::Message &message);
    fcitx::dbus::Message methodCall(const char *method);

    fcitx::dbus::Bus &bus_;
    DaemonCallbacks cb_;
    std::string owner_;
    fcitx::dbus::ServiceWatcher watcher_;
    std::unique_ptr<fcitx::dbus::ServiceWatcherEntry> watch_;
    std::unique_ptr<fcitx::dbus::Slot> match_;
    std::unique_ptr<fcitx::dbus::Slot> startCall_;
    std::unique_ptr<fcitx::dbus::Slot> stopCall_;
    std::unique_ptr<fcitx::dbus::Slot> cancelCall_;
};

} // namespace koeinput
