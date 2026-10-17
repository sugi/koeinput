#include "testsupport.h"
#include <testfrontend_public.h>
#include <fcitx-utils/capabilityflags.h>
#include <fcitx-utils/testing.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx/text.h>
#include <optional>
#include <string>
#include <vector>

using namespace fcitx;
using koeinput::test::failures;

namespace {

// ITestFrontend::sendKeyEvent exists only from fcitx5 5.1; this does the same on 5.0.
bool sendKey(Instance &instance, ICUUID uuid, const Key &key, bool isRelease) {
    auto *ic = instance.inputContextManager().findByUUID(uuid);
    if (!ic) return false;
    KeyEvent event(ic, key, isRelease);
    ic->keyEvent(event);
    return event.accepted();
}

} // namespace

int main() {
    setupTestingEnvironment(KOEINPUT_BINARY_DIR, {KOEINPUT_BINARY_DIR, FCITX5_SYSTEM_ADDONDIR},
                            {KOEINPUT_BINARY_DIR "/testdata", FCITX5_SYSTEM_PKGDATADIR "/testing",
                             FCITX5_SYSTEM_PKGDATADIR});
    char arg0[] = "addon_test";
    char arg1[] = "--disable=all";
    char arg2[] = "--enable=testfrontend,testim,testui,dbus,koeinput";
    char *argv[] = {arg0, arg1, arg2};
    Instance instance(3, argv);
    instance.addonManager().registerDefaultLoader(nullptr);

    AddonInstance *frontend = nullptr;
    ICUUID uuid{};
    std::vector<std::string> commits;
    auto commitWatch = instance.watchEvent(
        EventType::InputContextCommitString, EventWatcherPhase::Default, [&](Event &event) {
            commits.push_back(static_cast<CommitStringEvent &>(event).text());
        });
    auto send = [&](const char *key, bool release) { return sendKey(instance, uuid, Key(key), release); };
    auto ic = [&] { return instance.inputContextManager().findByUUID(uuid); };
    auto displayClear = [&] {
        auto &panel = ic()->inputPanel();
        KI_CHECK(panel.auxUp().empty());
        KI_CHECK(panel.preedit().empty());
        KI_CHECK(panel.clientPreedit().empty());
    };
    std::optional<koeinput::test::DaemonProcess> daemon;

    koeinput::test::Steps steps(instance.eventLoop(), [&] { instance.exit(); });
    steps
        .then(0, "addon loads",
              [&] {
                  KI_CHECK(instance.addonManager().addon("koeinput", true) != nullptr);
                  frontend = instance.addonManager().addon("testfrontend");
                  KI_CHECK(frontend != nullptr);
                  uuid = frontend->call<ITestFrontend::createInputContext>("testapp");
                  KI_CHECK(ic() != nullptr);
                  ic()->focusIn();
              })
        .then(0, "ordinary key passes",
              [&] {
                  KI_CHECK(!send("a", false));
                  KI_CHECK(!send("a", true));
              })
        // At startup fcitx5 shows the input method name in the aux text for about
        // a second (InputMethodGroupChanged); the scenarios need it gone.
        .then(2000, "startup input method information gone",
              [&] { KI_CHECK(ic()->inputPanel().auxUp().empty()); })
        .then(0, "no daemon: press", [&] { KI_CHECK(send("Alt_R", false)); })
        .then(300, "no daemon: release",
              [&] {
                  KI_CHECK(send("Alt+Alt_R", true));
                  KI_CHECK(commits.empty());
                  KI_CHECK(ic()->inputPanel().auxUp().empty());
              })
        .then(0, "composing passes through", [&] {
            ic()->inputPanel().setClientPreedit(Text("a"));
            KI_CHECK(!send("Alt_R", false));
            KI_CHECK(!send("Alt+Alt_R", true));
            ic()->inputPanel().setClientPreedit(Text());
        });

    if (koeinput::test::DaemonProcess::binary()) {
        steps
            .then(0, "hold: press",
                  [&] {
                      daemon.emplace();
                      KI_CHECK(send("Alt_R", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().toString().empty());
                  })
            .then(1400, "hold: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1500, "hold: committed",
                  [&] {
                      KI_CHECK(commits == std::vector<std::string>{"1"});
                      displayClear();
                  })
            .then(0, "release without modifier state: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(1400, "release without modifier state: release", [&] { KI_CHECK(send("Alt_R", true)); })
            .then(1500, "release without modifier state: committed",
                  [&] {
                      KI_CHECK(commits == std::vector<std::string>({"1", "1"}));
                      displayClear();
                  })
            .then(0, "tap: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(100, "tap: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1400, "tap: still recording, press again",
                  [&] {
                      KI_CHECK(commits.size() == 2);
                      KI_CHECK(send("Alt_R", false));
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1500, "tap: committed",
                  [&] {
                      KI_CHECK(commits == std::vector<std::string>({"1", "1", "1"}));
                      displayClear();
                  })
            .then(0, "esc: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(700, "esc: cancel",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1000, "esc: no commit",
                  [&] {
                      KI_CHECK(commits.size() == 3);
                      displayClear();
                  })
            .then(0, "other key: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(700, "other key: cancel",
                  [&] {
                      KI_CHECK(!send("x", false));
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1000, "other key: no commit",
                  [&] {
                      // x reached the test IM, which writes its own aux text.
                      KI_CHECK(commits.size() == 3);
                      KI_CHECK(ic()->inputPanel().preedit().empty());
                      KI_CHECK(ic()->inputPanel().clientPreedit().empty());
                  })
            .then(0, "partial: press",
                  [&] {
                      ic()->setCapabilityFlags(CapabilityFlag::Preedit);
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1300, "partial: in client preedit",
                  [&] {
                      KI_CHECK(!ic()->inputPanel().clientPreedit().empty());
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1500, "partial: committed",
                  [&] {
                      KI_CHECK(commits == std::vector<std::string>({"1", "1", "1", "1"}));
                      displayClear();
                  })
            .then(0, "daemon killed: press",
                  [&] {
                      daemon.reset();
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(300, "daemon killed: release",
                  [&] {
                      KI_CHECK(send("Alt+Alt_R", true));
                      KI_CHECK(commits.size() == 4);
                      displayClear();
                  })
            .then(0, "daemon restarted: press",
                  [&] {
                      daemon.emplace();
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1400, "daemon restarted: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1500, "daemon restarted: committed", [&] {
                KI_CHECK(commits == std::vector<std::string>({"1", "1", "1", "1", "1"}));
                displayClear();
            });
    } else {
        steps.then(0, "daemon scenarios skipped: KOEINPUT_TEST_DAEMON is not set", [] {});
    }
    steps.start();
    instance.exec();
    daemon.reset();
    std::fprintf(stderr, "%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
