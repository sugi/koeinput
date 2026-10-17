#include "testsupport.h"
#include <testfrontend_public.h>
#include <fcitx-config/rawconfig.h>
#include <fcitx-utils/capabilityflags.h>
#include <fcitx-utils/testing.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx/text.h>
#include <filesystem>
#include <fstream>
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
    // The old-config scenarios write fcitx5/conf/koeinput.conf here. The testing
    // environment skips the user config path, so the directory is also a system one.
    char configDir[] = "/tmp/koeinput-addon-test-XXXXXX";
    if (!mkdtemp(configDir)) return 1;
    setenv("XDG_CONFIG_HOME", configDir, 1);
    setenv("XDG_CONFIG_DIRS", configDir, 1);

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
    ICUUID uuid2{};
    std::vector<std::string> commits;
    auto commitWatch = instance.watchEvent(
        EventType::InputContextCommitString, EventWatcherPhase::Default, [&](Event &event) {
            commits.push_back(static_cast<CommitStringEvent &>(event).text());
        });
    auto send = [&](const char *key, bool release) { return sendKey(instance, uuid, Key(key), release); };
    auto ic = [&] { return instance.inputContextManager().findByUUID(uuid); };
    auto ic2 = [&] { return instance.inputContextManager().findByUUID(uuid2); };
    auto displayClear = [&](InputContext *context) {
        auto &panel = context->inputPanel();
        KI_CHECK(panel.auxUp().empty());
        KI_CHECK(panel.preedit().empty());
        KI_CHECK(panel.clientPreedit().empty());
    };
    // Every commit of the fake recognizer in these scenarios is "1".
    auto committed = [&](size_t n) {
        KI_CHECK(commits.size() == n);
        for (auto &c : commits) KI_CHECK(c == "1");
    };
    auto setOption = [&](const char *path, const char *value) {
        RawConfig raw;
        raw.setValueByPath(path, value);
        instance.addonManager().addon("koeinput")->setConfig(raw);
    };
    auto setHotkey = [&](const char *key) { setOption("Hotkey/0", key); };
    auto configValue = [&](const char *path) {
        RawConfig raw;
        instance.addonManager().addon("koeinput")->getConfig()->save(raw);
        return raw.valueByPath(path) ? *raw.valueByPath(path) : std::string("<unset>");
    };
    // Writes the config file as an older version would have, and reloads it.
    auto reloadWithFile = [&](const char *body) {
        const std::string dir = std::string(configDir) + "/fcitx5/conf";
        std::filesystem::create_directories(dir);
        std::ofstream out(dir + "/koeinput.conf");
        out << body;
        out.close();
        instance.addonManager().addon("koeinput")->reloadConfig();
    };
    auto cancelSession = [&](ICUUID target) {
        KI_CHECK(sendKey(instance, target, Key("Escape"), false));
        KI_CHECK(sendKey(instance, target, Key("Escape"), true));
        KI_CHECK(sendKey(instance, target, Key("Alt+Alt_R"), true));
    };
    auto onlyStatus = [&](InputContext *context) {
        auto &panel = context->inputPanel();
        KI_CHECK(!panel.auxUp().empty());
        KI_CHECK(panel.preedit().empty());
        KI_CHECK(panel.clientPreedit().empty());
    };
    auto inPanel = [&](InputContext *context) {
        KI_CHECK(!context->inputPanel().preedit().empty());
        KI_CHECK(context->inputPanel().clientPreedit().empty());
    };
    auto inClient = [&](InputContext *context) {
        KI_CHECK(!context->inputPanel().clientPreedit().empty());
        KI_CHECK(context->inputPanel().preedit().empty());
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
        .then(0, "no daemon: press",
              [&] {
                  KI_CHECK(send("Alt_R", false));
                  KI_CHECK(!ic()->inputPanel().auxUp().empty());
              })
        .then(300, "no daemon: release",
              [&] {
                  KI_CHECK(send("Alt+Alt_R", true));
                  KI_CHECK(commits.empty());
                  KI_CHECK(ic()->inputPanel().auxUp().empty());
              })
        .then(0, "composing passes through",
              [&] {
                  ic()->inputPanel().setClientPreedit(Text("a"));
                  KI_CHECK(!send("Alt_R", false));
                  KI_CHECK(!send("Alt+Alt_R", true));
                  ic()->inputPanel().setClientPreedit(Text());
              })
        .then(0, "old config: ShowPartial=False reads as Off",
              [&] {
                  KI_CHECK(configValue("PartialText") == "InApp");
                  reloadWithFile("ShowPartial=False\n");
                  KI_CHECK(configValue("PartialText") == "Off");
              })
        .then(0, "old config: ShowPartial=True reads as InApp",
              [&] {
                  reloadWithFile("ShowPartial=True\n");
                  KI_CHECK(configValue("PartialText") == "InApp");
              })
        .then(0, "old config: PartialText wins over ShowPartial", [&] {
            reloadWithFile("ShowPartial=False\nPartialText=Panel\n");
            KI_CHECK(configValue("PartialText") == "Panel");
            setOption("PartialText", "InApp");
            KI_CHECK(configValue("PartialText") == "InApp");
        });

    if (koeinput::test::DaemonProcess::binary()) {
        steps
            .then(0, "hold: press",
                  [&] {
                      daemon.emplace();
                      KI_CHECK(send("Alt_R", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().toString().empty());
                  })
            .then(1400, "hold: partial in the panel preedit, release",
                  [&] {
                      KI_CHECK(!ic()->inputPanel().preedit().empty());
                      KI_CHECK(ic()->inputPanel().clientPreedit().empty());
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1500, "hold: committed",
                  [&] {
                      committed(1);
                      displayClear(ic());
                  })
            .then(0, "second context: press",
                  [&] {
                      uuid2 = frontend->call<ITestFrontend::createInputContext>("testapp2");
                      KI_CHECK(ic2() != nullptr);
                      ic()->focusOut();
                      ic2()->focusIn();
                      KI_CHECK(sendKey(instance, uuid2, Key("Alt_R"), false));
                      KI_CHECK(!ic2()->inputPanel().auxUp().empty());
                      KI_CHECK(ic()->inputPanel().auxUp().empty());
                  })
            .then(1400, "second context: release",
                  [&] {
                      KI_CHECK(!ic2()->inputPanel().preedit().empty());
                      displayClear(ic());
                      KI_CHECK(sendKey(instance, uuid2, Key("Alt+Alt_R"), true));
                  })
            .then(1500, "second context: committed",
                  [&] {
                      committed(2);
                      displayClear(ic2());
                      displayClear(ic());
                      ic2()->focusOut();
                      ic()->focusIn();
                  })
            .then(0, "release without modifier state: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(1400, "release without modifier state: release", [&] { KI_CHECK(send("Alt_R", true)); })
            .then(1500, "release without modifier state: committed",
                  [&] {
                      committed(3);
                      displayClear(ic());
                  })
            .then(0, "tap: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(100, "tap: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1400, "tap: still recording, press again",
                  [&] {
                      committed(3);
                      KI_CHECK(send("Alt_R", false));
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1500, "tap: committed",
                  [&] {
                      committed(4);
                      displayClear(ic());
                  })
            .then(0, "esc: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(700, "esc: cancel",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1000, "esc: no commit",
                  [&] {
                      committed(4);
                      displayClear(ic());
                      KI_CHECK(!send("Escape", false));
                      KI_CHECK(!send("Escape", true));
                  })
            .then(0, "other key: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(700, "other key: cancel",
                  [&] {
                      KI_CHECK(!send("x", false));
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1000, "other key: no commit",
                  [&] {
                      committed(4);
                      displayClear(ic());
                  })
            .then(0, "partial: press",
                  [&] {
                      ic()->setCapabilityFlags(CapabilityFlag::Preedit);
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1300, "partial: in client preedit, focus out",
                  [&] {
                      KI_CHECK(!ic()->inputPanel().clientPreedit().empty());
                      KI_CHECK(ic()->inputPanel().preedit().empty());
                      ic()->focusOut();
                  })
            .then(500, "partial: focus out cancelled without a commit",
                  [&] {
                      committed(4);
                      displayClear(ic());
                      ic()->focusIn();
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(0, "partial: press again", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(1300, "partial: in client preedit, release",
                  [&] {
                      KI_CHECK(!ic()->inputPanel().clientPreedit().empty());
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1500, "partial: committed",
                  [&] {
                      committed(5);
                      displayClear(ic());
                  })
            // The context keeps the Preedit capability for the display-mode scenarios.
            .then(0, "panel mode: press",
                  [&] {
                      setOption("PartialText", "Panel");
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1300, "panel mode: in the panel preedit despite the Preedit capability",
                  [&] {
                      inPanel(ic());
                      cancelSession(uuid);
                  })
            .then(1000, "panel mode: cancelled",
                  [&] {
                      committed(5);
                      displayClear(ic());
                  })
            .then(0, "off: press",
                  [&] {
                      setOption("PartialText", "Off");
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1300, "off: status only",
                  [&] {
                      onlyStatus(ic());
                      cancelSession(uuid);
                  })
            .then(1000, "off: cancelled",
                  [&] {
                      committed(5);
                      displayClear(ic());
                  })
            .then(0, "app list with Off: press",
                  [&] {
                      setOption("PanelApps/0", "TESTAPP");
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1300, "app list with Off: in the panel preedit",
                  [&] {
                      inPanel(ic());
                      cancelSession(uuid);
                  })
            .then(1000, "app list with Off: cancelled",
                  [&] {
                      committed(5);
                      displayClear(ic());
                  })
            .then(0, "app list with InApp: press",
                  [&] {
                      setOption("PartialText", "InApp");
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1300, "app list with InApp: in the panel preedit",
                  [&] {
                      inPanel(ic());
                      cancelSession(uuid);
                  })
            .then(1000, "app list with InApp: cancelled",
                  [&] {
                      committed(5);
                      displayClear(ic());
                  })
            .then(0, "app not on the list: press in the second context",
                  [&] {
                      ic()->focusOut();
                      ic2()->setCapabilityFlags(CapabilityFlag::Preedit);
                      ic2()->focusIn();
                      KI_CHECK(sendKey(instance, uuid2, Key("Alt_R"), false));
                  })
            .then(1300, "app not on the list: in the client preedit",
                  [&] {
                      inClient(ic2());
                      displayClear(ic());
                      cancelSession(uuid2);
                  })
            .then(1000, "app not on the list: cancelled",
                  [&] {
                      committed(5);
                      displayClear(ic2());
                      ic2()->setCapabilityFlags(CapabilityFlags());
                      ic2()->focusOut();
                      ic()->setCapabilityFlags(CapabilityFlags());
                      ic()->focusIn();
                      // A key without children loads as an empty list.
                      setOption("PanelApps", "");
                      KI_CHECK(configValue("PanelApps/0") == "<unset>");
                  })
            .then(0, "plain hotkey: press Control+space",
                  [&] {
                      setHotkey("Control+space");
                      KI_CHECK(send("Control+space", false));
                  })
            .then(1400, "plain hotkey: release space without Control",
                  [&] { KI_CHECK(send("space", true)); })
            .then(1500, "plain hotkey: committed",
                  [&] {
                      committed(6);
                      displayClear(ic());
                  })
            // Control released first: the auto-repeat arrives as a bare space.
            .then(0, "plain hotkey repeat: press Control+space",
                  [&] { KI_CHECK(send("Control+space", false)); })
            .then(700, "plain hotkey repeat: bare space repeat is swallowed",
                  [&] {
                      KI_CHECK(send("space", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(700, "plain hotkey repeat: release", [&] { KI_CHECK(send("space", true)); })
            .then(1500, "plain hotkey repeat: committed",
                  [&] {
                      committed(7);
                      displayClear(ic());
                  })
            .then(0, "flagged repeat: press Control+space", [&] { KI_CHECK(send("Control+space", false)); })
            .then(1200, "flagged repeat: bare space with the Repeat state is swallowed",
                  [&] {
                      KI_CHECK(sendKey(instance, uuid, Key(FcitxKey_space, KeyStates({KeyState::Repeat})),
                                       false));
                      KI_CHECK(ic()->inputPanel().auxUp().toString() == "Recording");
                  })
            .then(300, "flagged repeat: release", [&] { KI_CHECK(send("space", true)); })
            .then(1500, "flagged repeat: committed",
                  [&] {
                      committed(8);
                      displayClear(ic());
                  })
            .then(0, "lost release: press Control+space", [&] { KI_CHECK(send("Control+space", false)); })
            .then(300, "lost release: esc cancels, no hotkey release",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(1100, "lost release: an ordinary space passes",
                  [&] {
                      KI_CHECK(!send("space", false));
                      KI_CHECK(!send("space", true));
                      committed(8);
                      displayClear(ic());
                  })
            .then(0, "tap with Control+space: press", [&] { KI_CHECK(send("Control+space", false)); })
            .then(100, "tap with Control+space: release space", [&] { KI_CHECK(send("space", true)); })
            .then(1300, "tap with Control+space: Control passes, the next press stops",
                  [&] {
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                      KI_CHECK(!send("Control_L", false));
                      KI_CHECK(!send("Control_L", true));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                      KI_CHECK(send("Control+space", false));
                      KI_CHECK(send("space", true));
                  })
            .then(1500, "tap with Control+space: committed",
                  [&] {
                      committed(9);
                      displayClear(ic());
                      setHotkey("Alt_R");
                  })
            // Real frontends give keycodes; Shift held at the release changes the
            // sym and the states, so the release pairs by keycode.
            .then(0, "keycode pairing: press Alt_R with a keycode",
                  [&] { KI_CHECK(sendKey(instance, uuid, Key(FcitxKey_Alt_R, KeyStates(), 108), false)); })
            .then(1400, "keycode pairing: release Alt+Shift+Meta_R with the same keycode",
                  [&] {
                      KI_CHECK(sendKey(instance, uuid,
                                       Key(FcitxKey_Meta_R, KeyStates({KeyState::Alt, KeyState::Shift}), 108),
                                       true));
                  })
            .then(1500, "keycode pairing: committed",
                  [&] {
                      committed(10);
                      displayClear(ic());
                  })
            // Without keycodes, Shift held at the release still pairs by sym.
            .then(0, "sym pairing: press Alt_R", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(1400, "sym pairing: release Alt+Shift+Alt_R", [&] { KI_CHECK(send("Alt+Shift+Alt_R", true)); })
            .then(1500, "sym pairing: committed",
                  [&] {
                      committed(11);
                      displayClear(ic());
                  })
            .then(0, "daemon killed: press",
                  [&] {
                      daemon.reset();
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(300, "daemon killed: release",
                  [&] {
                      KI_CHECK(send("Alt+Alt_R", true));
                      committed(11);
                      displayClear(ic());
                  })
            .then(0, "daemon restarted: press",
                  [&] {
                      daemon.emplace();
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1400, "daemon restarted: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1500, "daemon restarted: committed", [&] {
                committed(12);
                displayClear(ic());
            });
    } else {
        steps.then(0, "daemon scenarios skipped: KOEINPUT_TEST_DAEMON is not set", [] {});
    }
    steps.start();
    instance.exec();
    daemon.reset();
    std::filesystem::remove_all(configDir);
    std::fprintf(stderr, "%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
