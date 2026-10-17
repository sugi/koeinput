#include "../src/daemonclient.h"
#include "testsupport.h"
#include <fcitx-utils/dbus/message.h>
#include <fcitx-utils/event.h>
#include <algorithm>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <vector>

using namespace koeinput;
using koeinput::test::DaemonProcess;
using koeinput::test::failures;
using koeinput::test::Steps;

namespace {

std::string processExe(uint32_t pid) {
    char buf[4096];
    const std::string link = "/proc/" + std::to_string(pid) + "/exe";
    const ssize_t n = readlink(link.c_str(), buf, sizeof buf - 1);
    return n > 0 ? std::string(buf, n) : std::string();
}

// Needs a bus whose service directory holds an activation file for the test
// daemon (scripts/addon test sets one up). On such a bus every call to the
// daemon's name starts a daemon, so the other steps run on a bus without it.
int activationTest() {
    if (!std::getenv("DBUS_SESSION_BUS_ADDRESS")) {
        std::fprintf(stderr, "DBUS_SESSION_BUS_ADDRESS is not set; run the test under dbus-run-session\n");
        return 1;
    }
    fcitx::EventLoop loop;
    fcitx::dbus::Bus bus(fcitx::dbus::BusType::Session);
    bus.attachEventLoop(&loop);

    std::vector<std::string> log;
    DaemonCallbacks cb;
    cb.activateFailed = [&](const std::string &m) { log.push_back("activateFailed " + m); };
    cb.daemonLeft = [&] { log.push_back("left"); };
    DaemonClient client(bus, cb);

    // Independent of the client's own watcher. nullopt until it answers.
    std::optional<std::string> owner;
    uint64_t requestedUs = 0;
    uint64_t ownedUs = 0;
    fcitx::dbus::ServiceWatcher watcher(bus);
    auto watch = watcher.watchService(
        kBusName, [&](const std::string &, const std::string &, const std::string &newOwner) {
            owner = newOwner;
            if (!newOwner.empty() && !ownedUs) ownedUs = fcitx::now(CLOCK_MONOTONIC);
        });

    Steps steps(loop, [&] { loop.exit(); });
    steps
        .then(500, "activation: no daemon yet, activate",
              [&] {
                  KI_CHECK(owner && owner->empty());
                  requestedUs = fcitx::now(CLOCK_MONOTONIC);
                  client.activate();
              })
        .then(3000, "activation: the daemon owns its name, stop it",
              [&] {
                  for (auto &l : log) std::fprintf(stderr, "%s\n", l.c_str());
                  KI_CHECK(owner && !owner->empty());
                  if (!owner || owner->empty()) return;
                  std::fprintf(stderr, "the daemon owned its name %llu ms after activate()\n",
                               static_cast<unsigned long long>((ownedUs - requestedUs) / 1000));
                  auto m = bus.createMethodCall("org.freedesktop.DBus", "/org/freedesktop/DBus",
                                                "org.freedesktop.DBus", "GetConnectionUnixProcessID");
                  m << *owner;
                  auto reply = m.call(1000000);
                  uint32_t pid = 0;
                  if (!reply.isError()) reply >> pid;
                  KI_CHECK(pid > 0);
                  if (pid == 0) return;
                  // Only ever kill the test daemon.
                  const std::string exe = processExe(pid);
                  KI_CHECK(exe == DaemonProcess::binary());
                  if (exe == DaemonProcess::binary()) KI_CHECK(::kill(pid, SIGTERM) == 0);
              })
        .then(1000, "activation: the daemon is gone", [&] {
            KI_CHECK(owner && owner->empty());
            KI_CHECK(std::find(log.begin(), log.end(), "left") != log.end());
        });
    steps.start();
    loop.exec();
    std::fprintf(stderr, "%d failure(s)\n", failures);
    return failures ? 1 : 0;
}

} // namespace

int main(int argc, char **argv) {
    if (!DaemonProcess::binary()) {
        std::puts("skipped: KOEINPUT_TEST_DAEMON is not set");
        return 77;
    }
    if (argc > 1 && std::string(argv[1]) == "activation") return activationTest();
    fcitx::EventLoop loop;
    fcitx::dbus::Bus bus(fcitx::dbus::BusType::Session);
    bus.attachEventLoop(&loop);

    std::vector<std::string> log;
    std::optional<uint32_t> id;
    bool signalBeforeReply = false;
    // Set for the no-model daemon: DaemonProcess::notReady returns while it may still be loading.
    std::function<bool()> retryWhileLoading;
    DaemonCallbacks cb;
    cb.started = [&](uint32_t i) { id = i; log.push_back("started"); };
    cb.startFailed = [&](CallError k, const std::string &m) {
        if (retryWhileLoading && m.find("still loading") != std::string::npos && retryWhileLoading()) {
            return;
        }
        log.push_back("startFailed " + std::to_string(static_cast<int>(k)) + " " + m);
    };
    cb.stopFailed = [&](uint32_t i, CallError k, const std::string &) {
        log.push_back("stopFailed " + std::to_string(i) + " " + std::to_string(static_cast<int>(k)));
    };
    cb.stateChanged = [&](uint32_t, const std::string &s) {
        if (!id) signalBeforeReply = true;
        log.push_back("state " + s);
    };
    cb.partial = [&](uint32_t, const std::string &) { log.push_back("partial"); };
    cb.finalText = [&](uint32_t, const std::string &t) { log.push_back("final " + t); };
    cb.error = [&](uint32_t, const std::string &c, const std::string &) { log.push_back("error " + c); };
    cb.daemonLeft = [&] { log.push_back("left"); };
    DaemonClient client(bus, cb);

    auto has = [&](const std::string &entry) {
        for (auto &l : log) {
            if (l.rfind(entry, 0) == 0) return true;
        }
        return false;
    };
    std::optional<DaemonProcess> daemon;

    int loadingRetries = 0;
    std::unique_ptr<fcitx::EventSourceTime> retryTimer;
    auto retryStartLater = [&] {
        if (++loadingRetries > 30) return false;
        // Calling start() from inside its own reply callback would destroy the running slot.
        retryTimer = loop.addTimeEvent(CLOCK_MONOTONIC, fcitx::now(CLOCK_MONOTONIC) + 100000, 1,
                                       [&](fcitx::EventSourceTime *, uint64_t) {
                                           client.start();
                                           return true;
                                       });
        return true;
    };

    // A second connection that sends daemon-shaped signals the client must ignore.
    fcitx::dbus::Bus forger(fcitx::dbus::BusType::Session);
    forger.attachEventLoop(&loop);
    auto forgeState = [&](uint32_t sessionId, const std::string &state) {
        auto m = forger.createSignal(kObjectPath, kInterface, "StateChanged");
        m.setDestination(bus.uniqueName());
        m << sessionId << state;
        KI_CHECK(m.send());
    };

    Steps steps(loop, [&] { loop.exit(); });
    steps
        .then(0, "start without a daemon", [&] { client.start(); })
        .then(500, "no daemon reported",
              [&] {
                  KI_CHECK(has("startFailed " + std::to_string(static_cast<int>(CallError::NoDaemon))));
                  KI_CHECK(!has("left"));
                  log.clear();
                  // The watcher has answered "no owner": a forged signal is rejected.
                  forgeState(1, "forged");
                  daemon.emplace();
                  client.start();
              })
        .then(1250, "stop after 1.25 s",
              [&] {
                  KI_CHECK(has("started"));
                  KI_CHECK(has("state recording"));
                  KI_CHECK(id.has_value());
                  KI_CHECK(!has("left"));
                  KI_CHECK(!has("state forged"));
                  if (id) {
                      forgeState(*id, "forged");
                      client.stop(*id);
                  }
              })
        .then(1500, "final text arrived",
              [&] {
                  KI_CHECK(has("final 1"));
                  KI_CHECK(has("state done"));
                  KI_CHECK(!has("state forged"));
                  std::fprintf(stderr, "a signal preceded the Start reply: %s\n",
                               signalBeforeReply ? "yes" : "no");
                  log.clear();
                  id.reset();
                  client.start();
              })
        .then(800, "start again while recording",
              [&] {
                  KI_CHECK(id.has_value());
                  client.start();
              })
        .then(300, "busy reported, cancel the first session",
              [&] {
                  KI_CHECK(has("startFailed " + std::to_string(static_cast<int>(CallError::Busy))));
                  KI_CHECK(id.has_value());
                  if (id) client.cancel(*id);
              })
        .then(500, "cancelled without final",
              [&] {
                  KI_CHECK(has("state cancelled"));
                  KI_CHECK(!has("final"));
                  log.clear();
                  client.stop(999);
              })
        .then(300, "stop of an unknown session fails",
              [&] {
                  KI_CHECK(has("stopFailed 999 " + std::to_string(static_cast<int>(CallError::Other))));
                  log.clear();
                  daemon.reset();
              })
        .then(300, "daemon left",
              [&] {
                  KI_CHECK(has("left"));
                  log.clear();
                  id.reset();
                  // No owner now: a daemon-shaped signal must be rejected.
                  forgeState(1, "forged");
                  daemon.emplace(std::vector<std::pair<std::string, std::string>>{
                      {"KOEINPUT_TEST_PANIC", "1"}});
                  client.start();
              })
        .then(1500, "recognizer panic reported",
              [&] {
                  KI_CHECK(has("started"));
                  KI_CHECK(has("error internal"));
                  KI_CHECK(has("state error"));
                  KI_CHECK(!has("state forged"));
                  log.clear();
                  daemon.reset();
                  daemon.emplace(DaemonProcess::notReady({{"KOEINPUT_TEST_NO_MODEL", "1"}}));
                  retryWhileLoading = retryStartLater;
                  client.start();
              })
        .then(3500, "no model", [&] {
            std::fprintf(stderr, "retries while the model was loading: %d\n", loadingRetries);
            KI_CHECK(has("startFailed " + std::to_string(static_cast<int>(CallError::NoModel))));
            KI_CHECK(has("startFailed " + std::to_string(static_cast<int>(CallError::NoModel)) +
                         " no model installed"));
        });
    steps.start();
    loop.exec();
    std::fprintf(stderr, "%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
