#include "daemonclient.h"
#include <fcitx-utils/dbus/matchrule.h>
#include <fcitx-utils/dbus/message.h>
#include <utility>

namespace koeinput {

using fcitx::dbus::Message;

DaemonClient::DaemonClient(fcitx::dbus::Bus &bus, DaemonCallbacks callbacks)
    : bus_(bus), cb_(std::move(callbacks)), watcher_(bus) {
    watch_ = watcher_.watchService(
        kBusName, [this](const std::string &, const std::string &oldOwner,
                         const std::string &newOwner) {
            if (!oldOwner.empty() && newOwner.empty()) cb_.daemonLeft();
        });
    match_ = bus_.addMatch(fcitx::dbus::MatchRule(kBusName, kObjectPath, kInterface),
                           [this](Message &message) { return onSignal(message); });
}

DaemonClient::~DaemonClient() = default;

Message DaemonClient::methodCall(const char *method) {
    return bus_.createMethodCall(kBusName, kObjectPath, kInterface, method);
}

void DaemonClient::start() {
    auto message = methodCall("Start");
    // An empty a{sv}, written without dbus::Variant: fcitx5 5.0 does not install
    // fcitx-utils/dbus/variant.h, so Variant is an incomplete type there.
    message << fcitx::dbus::Container(fcitx::dbus::Container::Type::Array,
                                      fcitx::dbus::Signature("{sv}"))
            << fcitx::dbus::ContainerEnd();
    startCall_ = message.callAsync(kStartTimeoutUsec, [this](Message &reply) {
        if (reply.isError()) {
            cb_.startFailed(callErrorFromName(reply.errorName()), reply.errorMessage());
            return true;
        }
        uint32_t id = 0;
        reply >> id;
        if (!reply) {
            cb_.startFailed(CallError::Other, "unexpected reply to Start");
            return true;
        }
        cb_.started(id);
        return true;
    });
}

void DaemonClient::stop(uint32_t id) {
    auto message = methodCall("Stop");
    message << id;
    stopCall_ = message.callAsync(kCallTimeoutUsec, [this, id](Message &reply) {
        if (reply.isError()) {
            cb_.stopFailed(id, callErrorFromName(reply.errorName()), reply.errorMessage());
        }
        return true;
    });
}

void DaemonClient::cancel(uint32_t id) {
    auto message = methodCall("Cancel");
    message << id;
    cancelCall_ = message.callAsync(kCallTimeoutUsec, [](Message &) { return true; });
}

bool DaemonClient::onSignal(Message &message) {
    if (message.type() != fcitx::dbus::MessageType::Signal) return false;
    const std::string member = message.member();
    uint32_t id = 0;
    std::string a;
    if (member == "StateChanged") {
        message >> id >> a;
        if (message) cb_.stateChanged(id, a);
    } else if (member == "Partial") {
        message >> id >> a;
        if (message) cb_.partial(id, a);
    } else if (member == "Final") {
        message >> id >> a;
        if (message) cb_.finalText(id, a);
    } else if (member == "Error") {
        std::string b;
        message >> id >> a >> b;
        if (message) cb_.error(id, a, b);
    }
    return true;
}

} // namespace koeinput
