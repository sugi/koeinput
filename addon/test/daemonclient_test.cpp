#include "../src/daemonclient.h"
#include "testsupport.h"
#include <fcitx-utils/event.h>
#include <optional>
#include <string>
#include <vector>

using namespace koeinput;
using koeinput::test::DaemonProcess;
using koeinput::test::failures;
using koeinput::test::Steps;

int main() {
    if (!DaemonProcess::binary()) {
        std::puts("skipped: KOEINPUT_TEST_DAEMON is not set");
        return 77;
    }
    fcitx::EventLoop loop;
    fcitx::dbus::Bus bus(fcitx::dbus::BusType::Session);
    bus.attachEventLoop(&loop);

    std::vector<std::string> log;
    std::optional<uint32_t> id;
    bool signalBeforeReply = false;
    DaemonCallbacks cb;
    cb.started = [&](uint32_t i) { id = i; log.push_back("started"); };
    cb.startFailed = [&](CallError k, const std::string &m) {
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

    Steps steps(loop, [&] { loop.exit(); });
    steps
        .then(0, "start without a daemon", [&] { client.start(); })
        .then(500, "no daemon reported",
              [&] {
                  KI_CHECK(has("startFailed " + std::to_string(static_cast<int>(CallError::NoDaemon))));
                  log.clear();
                  daemon.emplace();
                  client.start();
              })
        .then(1400, "stop after 1.4 s",
              [&] {
                  KI_CHECK(has("started"));
                  KI_CHECK(has("state recording"));
                  KI_CHECK(id.has_value());
                  if (id) client.stop(*id);
              })
        .then(1500, "final text arrived",
              [&] {
                  KI_CHECK(has("final 1"));
                  KI_CHECK(has("state done"));
                  std::fprintf(stderr, "a signal preceded the Start reply: %s\n",
                               signalBeforeReply ? "yes" : "no");
                  log.clear();
                  id.reset();
                  client.start();
              })
        .then(800, "cancel while recording",
              [&] {
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
                  daemon.emplace(DaemonProcess::notReady({{"KOEINPUT_TEST_NO_MODEL", "1"}}));
                  client.start();
              })
        .then(500, "no model", [&] {
            KI_CHECK(has("startFailed " + std::to_string(static_cast<int>(CallError::NoModel))));
            KI_CHECK(has("startFailed " + std::to_string(static_cast<int>(CallError::NoModel)) +
                         " no model installed"));
        });
    steps.start();
    loop.exec();
    std::fprintf(stderr, "%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
