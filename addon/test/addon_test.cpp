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
    // The same key as a real frontend sends it: with the keycode of the physical key.
    auto sendCode = [&](const char *key, int code, bool release) {
        const Key k(key);
        return sendKey(instance, uuid, Key(k.sym(), k.states(), code), release);
    };
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
    auto setHotkeys = [&](const char *first, const char *second) {
        RawConfig raw;
        raw.setValueByPath("Hotkey/0", first);
        raw.setValueByPath("Hotkey/1", second);
        instance.addonManager().addon("koeinput")->setConfig(raw);
    };
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
            .then(0, "app list with Off, app not on the list: press in the second context",
                  [&] {
                      ic()->focusOut();
                      ic2()->setCapabilityFlags(CapabilityFlag::Preedit);
                      ic2()->focusIn();
                      KI_CHECK(sendKey(instance, uuid2, Key("Alt_R"), false));
                  })
            .then(1300, "app list with Off, app not on the list: status only",
                  [&] {
                      onlyStatus(ic2());
                      displayClear(ic());
                      cancelSession(uuid2);
                  })
            .then(1000, "app list with Off, app not on the list: cancelled",
                  [&] {
                      committed(5);
                      displayClear(ic2());
                      ic2()->setCapabilityFlags(CapabilityFlags());
                      ic2()->focusOut();
                      ic()->focusIn();
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
            .then(0, "altgr: press", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(700, "altgr: ISO_Level3_Shift and Mode_switch do not cancel",
                  [&] {
                      KI_CHECK(!send("ISO_Level3_Shift", false));
                      KI_CHECK(!send("ISO_Level3_Shift", true));
                      KI_CHECK(!send("Mode_switch", false));
                      KI_CHECK(!send("Mode_switch", true));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(700, "altgr: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1500, "altgr: committed",
                  [&] {
                      committed(12);
                      displayClear(ic());
                  })
            // AltGr never auto-repeats, so a press while it counts as down is a new press.
            .then(0, "altgr hotkey: press",
                  [&] {
                      setHotkey("ISO_Level3_Shift");
                      KI_CHECK(send("ISO_Level3_Shift", false));
                  })
            .then(300, "altgr hotkey: esc cancels, the release is lost",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(300, "altgr hotkey: a press within the repeat gap starts a new session",
                  [&] {
                      KI_CHECK(send("ISO_Level3_Shift", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(1400, "altgr hotkey: release", [&] { KI_CHECK(send("ISO_Level3_Shift", true)); })
            .then(1500, "altgr hotkey: committed",
                  [&] {
                      committed(13);
                      displayClear(ic());
                      setHotkey("Alt_R");
                  })
            // With Shift held, the Alt_R key produces Meta_R: another key on the same keycode.
            .then(0, "lost modifier release: press Alt_R with a keycode",
                  [&] { KI_CHECK(sendKey(instance, uuid, Key(FcitxKey_Alt_R, KeyStates(), 108), false)); })
            .then(300, "lost modifier release: esc cancels, the release is lost",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(1100, "lost modifier release: Shift+Meta_R on the same keycode passes with its release",
                  [&] {
                      const Key shiftMeta(FcitxKey_Meta_R, KeyStates({KeyState::Shift}), 108);
                      KI_CHECK(!send("Shift_L", false));
                      KI_CHECK(!sendKey(instance, uuid, shiftMeta, false));
                      KI_CHECK(!sendKey(instance, uuid, shiftMeta, true));
                      KI_CHECK(!send("Shift_L", true));
                      committed(13);
                      displayClear(ic());
                  })
            // Without the Alt state, Control+space is no evidence that Alt_R is still down.
            .then(0, "two hotkeys, lost release: press Alt_R",
                  [&] {
                      setHotkeys("Alt_R", "Control+space");
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(300, "two hotkeys, lost release: esc cancels, the release is lost",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(1100, "two hotkeys, lost release: Control+space starts a new session",
                  [&] {
                      KI_CHECK(send("Control+space", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(1400, "two hotkeys, lost release: release", [&] { KI_CHECK(send("space", true)); })
            .then(1500, "two hotkeys, lost release: committed",
                  [&] {
                      committed(14);
                      displayClear(ic());
                  })
            .then(0, "chord: press Alt_R",
                  [&] {
                      setHotkeys("Alt_R", "Alt+space");
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(700, "chord: Alt+space while Alt_R is down is swallowed with its release",
                  [&] {
                      KI_CHECK(send("Alt+space", false));
                      KI_CHECK(send("Alt+space", true));
                  })
            .then(600, "chord: still recording, release Alt_R",
                  [&] {
                      KI_CHECK(ic()->inputPanel().auxUp().toString() == "Recording");
                      KI_CHECK(send("Alt+Alt_R", true));
                  })
            .then(1500, "chord: committed",
                  [&] {
                      committed(15);
                      displayClear(ic());
                      KI_CHECK(!send("space", false));
                      KI_CHECK(!send("space", true));
                  })
            // Alt_R released first while space stays down: its bare repeats and
            // its release belong to the chord.
            .then(0, "chord repeat: press Alt_R", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(700, "chord repeat: Alt+space", [&] { KI_CHECK(send("Alt+space", false)); })
            .then(700, "chord repeat: release Alt_R, a bare space repeat is swallowed",
                  [&] {
                      KI_CHECK(send("Alt+Alt_R", true));
                      KI_CHECK(send("space", false));
                  })
            .then(500, "chord repeat: later repeats are swallowed",
                  [&] {
                      KI_CHECK(sendKey(instance, uuid, Key(FcitxKey_space, KeyStates({KeyState::Repeat})),
                                       false));
                      KI_CHECK(send("space", false));
                  })
            .then(500, "chord repeat: the space release is swallowed", [&] { KI_CHECK(send("space", true)); })
            .then(1500, "chord repeat: committed",
                  [&] {
                      committed(16);
                      displayClear(ic());
                      KI_CHECK(!send("space", false));
                      KI_CHECK(!send("space", true));
                  })
            // A press without the Alt state shows that Alt_R is not held any more.
            .then(0, "shared modifier, lost release: press Alt_R", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(300, "shared modifier, lost release: esc without the Alt state cancels",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(1100, "shared modifier, lost release: Alt+space starts a new session",
                  [&] {
                      KI_CHECK(send("Alt+space", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(1400, "shared modifier, lost release: release", [&] { KI_CHECK(send("space", true)); })
            .then(1500, "shared modifier, lost release: committed",
                  [&] {
                      committed(17);
                      displayClear(ic());
                  })
            .then(0, "chord timeout: press Alt_R", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(700, "chord timeout: Alt+space", [&] { KI_CHECK(send("Alt+space", false)); })
            .then(700, "chord timeout: release Alt_R", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1200, "chord timeout: a bare space after a long gap is an ordinary key",
                  [&] {
                      KI_CHECK(!send("space", false));
                      KI_CHECK(!send("space", true));
                  })
            .then(1500, "chord timeout: committed",
                  [&] {
                      committed(18);
                      displayClear(ic());
                  })
            // A plain first hotkey has no modifier state: it counts as held while it repeats.
            .then(0, "plain first hotkey: press Control+space",
                  [&] {
                      setHotkeys("Control+space", "Alt_R");
                      KI_CHECK(send("Control+space", false));
                  })
            .then(500, "plain first hotkey: Alt_R within the repeat gap is a chord",
                  [&] {
                      KI_CHECK(send("Alt_R", false));
                      KI_CHECK(send("Alt+Alt_R", true));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(900, "plain first hotkey: release space", [&] { KI_CHECK(send("space", true)); })
            .then(1500, "plain first hotkey: committed",
                  [&] {
                      committed(19);
                      displayClear(ic());
                  })
            .then(0, "plain first hotkey, lost release: press Control+space",
                  [&] { KI_CHECK(send("Control+space", false)); })
            .then(300, "plain first hotkey, lost release: esc cancels",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(1100, "plain first hotkey, lost release: Alt_R after the gap starts a new session",
                  [&] {
                      KI_CHECK(send("Alt_R", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(1400, "plain first hotkey, lost release: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1500, "plain first hotkey, lost release: committed",
                  [&] {
                      committed(20);
                      displayClear(ic());
                  })
            // AltGr is held as Mod5: a press without it, even within a second, shows it is up.
            .then(0, "altgr evidence: press ISO_Level3_Shift",
                  [&] {
                      setHotkeys("ISO_Level3_Shift", "Alt_R");
                      KI_CHECK(send("ISO_Level3_Shift", false));
                  })
            .then(300, "altgr evidence: esc without Mod5 cancels",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(300, "altgr evidence: Alt_R within the repeat gap starts a new session",
                  [&] {
                      KI_CHECK(send("Alt_R", false));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(1400, "altgr evidence: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1500, "altgr evidence: committed",
                  [&] {
                      committed(21);
                      displayClear(ic());
                  })
            // A press carrying Mod5 is evidence that AltGr is still held.
            .then(0, "altgr held: press ISO_Level3_Shift",
                  [&] {
                      setHotkeys("ISO_Level3_Shift", "Control+space");
                      KI_CHECK(send("ISO_Level3_Shift", false));
                  })
            .then(700, "altgr held: Control+space with Mod5 is a chord",
                  [&] {
                      const Key chord(FcitxKey_space, KeyStates({KeyState::Ctrl, KeyState::Mod5}));
                      KI_CHECK(sendKey(instance, uuid, chord, false));
                      KI_CHECK(sendKey(instance, uuid, chord, true));
                  })
            .then(600, "altgr held: still recording, release ISO_Level3_Shift",
                  [&] {
                      KI_CHECK(ic()->inputPanel().auxUp().toString() == "Recording");
                      KI_CHECK(send("ISO_Level3_Shift", true));
                  })
            .then(1500, "altgr held: committed",
                  [&] {
                      committed(22);
                      displayClear(ic());
                      setHotkey("Alt_R");
                  })
            // After a lost Alt_R release, Control+Alt_R belongs to the app: so does its release.
            .then(0, "lost release, other combination: press Alt_R", [&] { KI_CHECK(send("Alt_R", false)); })
            .then(300, "lost release, other combination: esc cancels",
                  [&] {
                      KI_CHECK(send("Escape", false));
                      KI_CHECK(send("Escape", true));
                  })
            .then(1100, "lost release, other combination: Control+Alt_R passes with its release",
                  [&] {
                      KI_CHECK(!send("Control_L", false));
                      KI_CHECK(!send("Control+Alt_R", false));
                      KI_CHECK(!send("Control+Alt+Alt_R", true));
                      KI_CHECK(!send("Control_L", true));
                      committed(22);
                      displayClear(ic());
                  })
            // Control+Alt_R during a held Control+space session is not the Alt_R hotkey.
            .then(0, "other combination mid-hold: press Control+space",
                  [&] {
                      setHotkeys("Control+space", "Alt_R");
                      KI_CHECK(send("Control+space", false));
                  })
            .then(700, "other combination mid-hold: Control+Alt_R passes, the session goes on",
                  [&] {
                      KI_CHECK(!send("Control+Alt_R", false));
                      KI_CHECK(!send("Control+Alt+Alt_R", true));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(700, "other combination mid-hold: release space", [&] { KI_CHECK(send("space", true)); })
            .then(1500, "other combination mid-hold: committed",
                  [&] {
                      committed(23);
                      displayClear(ic());
                      setHotkey("Alt_R");
                  })
            // With keycodes (keycode 108 is the Alt_R key, 65 space, 37 Control_L,
            // 9 Escape), every release pairs with the press of its keycode.
            .then(0, "keycodes, hold: press", [&] { KI_CHECK(sendCode("Alt_R", 108, false)); })
            .then(1400, "keycodes, hold: a release without a press passes, the hotkey release stops",
                  [&] {
                      KI_CHECK(!sendCode("space", 65, true));
                      KI_CHECK(ic()->inputPanel().auxUp().toString() == "Recording");
                      KI_CHECK(sendCode("Alt+Alt_R", 108, true));
                  })
            .then(1500, "keycodes, hold: committed",
                  [&] {
                      committed(24);
                      displayClear(ic());
                  })
            .then(0, "keycodes, tap: press and release",
                  [&] {
                      KI_CHECK(sendCode("Alt_R", 108, false));
                      KI_CHECK(sendCode("Alt+Alt_R", 108, true));
                  })
            .then(1400, "keycodes, tap: still recording, press again",
                  [&] {
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                      KI_CHECK(sendCode("Alt_R", 108, false));
                      KI_CHECK(sendCode("Alt+Alt_R", 108, true));
                  })
            .then(1500, "keycodes, tap: committed",
                  [&] {
                      committed(25);
                      displayClear(ic());
                  })
            .then(0, "keycodes, esc: press", [&] { KI_CHECK(sendCode("Alt_R", 108, false)); })
            .then(300, "keycodes, esc: cancel, the release is lost",
                  [&] {
                      KI_CHECK(sendCode("Escape", 9, false));
                      KI_CHECK(sendCode("Escape", 9, true));
                      displayClear(ic());
                  })
            // A new press of the hotkey's keycode replaces the stored press.
            .then(300, "keycodes, esc: a press within the repeat gap starts a new session",
                  [&] {
                      KI_CHECK(sendCode("Alt_R", 108, false));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(1400, "keycodes, esc: release", [&] { KI_CHECK(sendCode("Alt+Alt_R", 108, true)); })
            .then(1500, "keycodes, esc: committed, a release without a press passes",
                  [&] {
                      committed(26);
                      displayClear(ic());
                      KI_CHECK(!sendCode("Alt+Alt_R", 108, true));
                  })
            // After a lost Alt_R release, Control+Alt_R on the same keycode is a
            // new key: it passes with its release, whichever modifier goes up first.
            .then(0, "keycodes, lost release: press Alt_R", [&] { KI_CHECK(sendCode("Alt_R", 108, false)); })
            .then(300, "keycodes, lost release: esc cancels",
                  [&] {
                      KI_CHECK(sendCode("Escape", 9, false));
                      KI_CHECK(sendCode("Escape", 9, true));
                  })
            .then(300, "keycodes, lost release: Control+Alt_R within the repeat gap passes, Control released first",
                  [&] {
                      KI_CHECK(!sendCode("Control_L", 37, false));
                      KI_CHECK(!sendCode("Control+Alt_R", 108, false));
                      KI_CHECK(!sendCode("Control+Alt+Control_L", 37, true));
                      KI_CHECK(!sendCode("Alt+Alt_R", 108, true));
                      committed(26);
                      displayClear(ic());
                  })
            .then(0, "keycodes, lost release, after the gap: press Alt_R",
                  [&] { KI_CHECK(sendCode("Alt_R", 108, false)); })
            .then(300, "keycodes, lost release, after the gap: esc cancels",
                  [&] {
                      KI_CHECK(sendCode("Escape", 9, false));
                      KI_CHECK(sendCode("Escape", 9, true));
                  })
            .then(1100, "keycodes, lost release, after the gap: Control+Alt_R passes, Alt_R released first",
                  [&] {
                      KI_CHECK(!sendCode("Control_L", 37, false));
                      KI_CHECK(!sendCode("Control+Alt_R", 108, false));
                      KI_CHECK(!sendCode("Control+Alt+Alt_R", 108, true));
                      KI_CHECK(!sendCode("Control+Control_L", 37, true));
                      committed(26);
                      displayClear(ic());
                  })
            // Control+Alt_R during a held Control+space session belongs to the app;
            // with Control released first its release arrives as Alt+Alt_R.
            .then(0, "keycodes, mid-hold: press Control+space",
                  [&] {
                      setHotkeys("Control+space", "Alt_R");
                      KI_CHECK(!sendCode("Control_L", 37, false));
                      KI_CHECK(sendCode("Control+space", 65, false));
                  })
            .then(500, "keycodes, mid-hold: Control+Alt_R passes, Control released first, the session goes on",
                  [&] {
                      KI_CHECK(!sendCode("Control+Alt_R", 108, false));
                      KI_CHECK(!sendCode("Control+Alt+Control_L", 37, true));
                      KI_CHECK(!sendCode("Alt+Alt_R", 108, true));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(900, "keycodes, mid-hold: release space", [&] { KI_CHECK(sendCode("space", 65, true)); })
            .then(1500, "keycodes, mid-hold: committed",
                  [&] {
                      committed(27);
                      displayClear(ic());
                  })
            .then(0, "keycodes, chord: press Alt_R",
                  [&] {
                      setHotkeys("Alt_R", "Alt+space");
                      KI_CHECK(sendCode("Alt_R", 108, false));
                  })
            .then(700, "keycodes, chord: Alt+space while Alt_R is down is swallowed with its release",
                  [&] {
                      KI_CHECK(sendCode("Alt+space", 65, false));
                      KI_CHECK(sendCode("Alt+space", 65, true));
                  })
            .then(600, "keycodes, chord: still recording, release Alt_R",
                  [&] {
                      KI_CHECK(ic()->inputPanel().auxUp().toString() == "Recording");
                      KI_CHECK(sendCode("Alt+Alt_R", 108, true));
                  })
            .then(1500, "keycodes, chord: committed",
                  [&] {
                      committed(28);
                      displayClear(ic());
                      KI_CHECK(!sendCode("space", 65, false));
                      KI_CHECK(!sendCode("space", 65, true));
                  })
            .then(0, "keycodes, chord repeat: press Alt_R", [&] { KI_CHECK(sendCode("Alt_R", 108, false)); })
            .then(700, "keycodes, chord repeat: Alt+space", [&] { KI_CHECK(sendCode("Alt+space", 65, false)); })
            .then(700, "keycodes, chord repeat: release Alt_R, a bare space repeat is swallowed",
                  [&] {
                      KI_CHECK(sendCode("Alt+Alt_R", 108, true));
                      KI_CHECK(sendCode("space", 65, false));
                  })
            .then(500, "keycodes, chord repeat: later repeats are swallowed",
                  [&] {
                      KI_CHECK(sendKey(instance, uuid, Key(FcitxKey_space, KeyStates({KeyState::Repeat}), 65),
                                       false));
                      KI_CHECK(sendCode("space", 65, false));
                  })
            .then(500, "keycodes, chord repeat: the space release is swallowed",
                  [&] { KI_CHECK(sendCode("space", 65, true)); })
            .then(1500, "keycodes, chord repeat: committed",
                  [&] {
                      committed(29);
                      displayClear(ic());
                      KI_CHECK(!sendCode("space", 65, false));
                      KI_CHECK(!sendCode("space", 65, true));
                  })
            // A key the app got keeps its repeats and its release, whatever
            // they would be classified as now.
            .then(0, "keycodes, held key then hotkey: space, then Alt_R",
                  [&] {
                      KI_CHECK(!sendCode("space", 65, false));
                      KI_CHECK(sendCode("Alt_R", 108, false));
                  })
            .then(500, "keycodes, held key then hotkey: the Alt+space repeats and the release pass",
                  [&] {
                      KI_CHECK(!sendCode("Alt+space", 65, false));
                      KI_CHECK(!sendKey(instance, uuid, Key(FcitxKey_space, KeyStates({KeyState::Alt, KeyState::Repeat}), 65),
                                        false));
                      KI_CHECK(!sendCode("Alt+space", 65, true));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(900, "keycodes, held key then hotkey: release Alt_R",
                  [&] { KI_CHECK(sendCode("Alt+Alt_R", 108, true)); })
            .then(1500, "keycodes, held key then hotkey: committed",
                  [&] {
                      committed(30);
                      displayClear(ic());
                      setHotkey("Control+space");
                  })
            .then(0, "keycodes, rolled hotkey: space, then Control",
                  [&] {
                      KI_CHECK(!sendCode("space", 65, false));
                      KI_CHECK(!sendCode("Control_L", 37, false));
                  })
            .then(500, "keycodes, rolled hotkey: the Control+space repeat and the releases pass",
                  [&] {
                      KI_CHECK(!sendCode("Control+space", 65, false));
                      KI_CHECK(ic()->inputPanel().auxUp().empty());
                      KI_CHECK(!sendCode("Control+space", 65, true));
                      KI_CHECK(!sendCode("Control+Control_L", 37, true));
                      committed(30);
                      setHotkey("Alt_R");
                  })
            .then(0, "keycodes, held escape then hotkey: Escape, then Alt_R",
                  [&] {
                      KI_CHECK(!sendCode("Escape", 9, false));
                      KI_CHECK(sendCode("Alt_R", 108, false));
                  })
            .then(500, "keycodes, held escape then hotkey: the Escape repeat and release pass, the session goes on",
                  [&] {
                      KI_CHECK(!sendCode("Alt+Escape", 9, false));
                      KI_CHECK(!sendCode("Alt+Escape", 9, true));
                      KI_CHECK(!ic()->inputPanel().auxUp().empty());
                  })
            .then(900, "keycodes, held escape then hotkey: release Alt_R",
                  [&] { KI_CHECK(sendCode("Alt+Alt_R", 108, true)); })
            .then(1500, "keycodes, held escape then hotkey: committed",
                  [&] {
                      committed(31);
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
                      committed(31);
                      displayClear(ic());
                  })
            .then(0, "daemon restarted: press",
                  [&] {
                      daemon.emplace();
                      KI_CHECK(send("Alt_R", false));
                  })
            .then(1400, "daemon restarted: release", [&] { KI_CHECK(send("Alt+Alt_R", true)); })
            .then(1500, "daemon restarted: committed", [&] {
                committed(32);
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
