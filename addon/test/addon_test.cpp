#include "testsupport.h"
#include <testfrontend_public.h>
#include <fcitx-utils/testing.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/instance.h>

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
    koeinput::test::Steps steps(instance.eventLoop(), [&] { instance.exit(); });
    steps
        .then(0, "addon loads",
              [&] {
                  KI_CHECK(instance.addonManager().addon("koeinput", true) != nullptr);
                  frontend = instance.addonManager().addon("testfrontend");
                  KI_CHECK(frontend != nullptr);
                  uuid = frontend->call<ITestFrontend::createInputContext>("testapp");
                  auto *ic = instance.inputContextManager().findByUUID(uuid);
                  KI_CHECK(ic != nullptr);
                  ic->focusIn();
              })
        .then(0, "ordinary key passes", [&] {
            KI_CHECK(!sendKey(instance, uuid, Key("a"), false));
            KI_CHECK(!sendKey(instance, uuid, Key("a"), true));
        });
    steps.start();
    instance.exec();
    std::fprintf(stderr, "%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
