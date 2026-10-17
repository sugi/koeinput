#include "../src/controller.h"
#include <cstdio>
#include <string>
#include <vector>

using namespace koeinput;
using Log = std::vector<std::string>;

namespace {

int failures = 0;

void checkLog(const char *file, int line, const Log &actual, const Log &expected) {
    if (actual == expected) return;
    ++failures;
    std::fprintf(stderr, "%s:%d: log mismatch\n  actual:  ", file, line);
    for (auto &s : actual) std::fprintf(stderr, "[%s] ", s.c_str());
    std::fprintf(stderr, "\n  expected:");
    for (auto &s : expected) std::fprintf(stderr, " [%s]", s.c_str());
    std::fprintf(stderr, "\n");
}
#define CHECK_LOG(actual, ...) checkLog(__FILE__, __LINE__, (actual), Log{__VA_ARGS__})
#define CHECK(cond)                                                                 \
    do {                                                                            \
        if (!(cond)) {                                                              \
            ++failures;                                                             \
            std::fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #cond); \
        }                                                                           \
    } while (0)

const char *statusName(Status s) {
    switch (s) {
    case Status::Preparing: return "Preparing";
    case Status::Recording: return "Recording";
    case Status::RecordingTap: return "RecordingTap";
    case Status::Finalizing: return "Finalizing";
    }
    return "?";
}

const char *noticeName(Notice n) {
    switch (n) {
    case Notice::NoDaemon: return "NoDaemon";
    case Notice::NoModel: return "NoModel";
    case Notice::Busy: return "Busy";
    case Notice::NoResponse: return "NoResponse";
    case Notice::AudioError: return "AudioError";
    case Notice::ModelError: return "ModelError";
    case Notice::InternalError: return "InternalError";
    case Notice::DaemonLeft: return "DaemonLeft";
    case Notice::Failed: return "Failed";
    }
    return "?";
}

struct FakeActions : Actions {
    Log log;
    void callStart() override { log.push_back("start"); }
    void callStop(uint32_t id) override { log.push_back("stop " + std::to_string(id)); }
    void callCancel(uint32_t id) override { log.push_back("cancel " + std::to_string(id)); }
    void showStatus(Status s) override { log.push_back(std::string("status ") + statusName(s)); }
    void showPartial(const std::string &t) override { log.push_back("partial " + t); }
    void clearDisplay() override { log.push_back("clear"); }
    void commit(const std::string &t) override { log.push_back("commit " + t); }
    void notify(Notice n, const std::string &d) override {
        log.push_back(std::string("notify ") + noticeName(n) + (d.empty() ? "" : " " + d));
    }
    void startTimer(uint64_t ms) override { log.push_back("timer " + std::to_string(ms)); }
    void stopTimer() override { log.push_back("untimer"); }
    Log take() {
        Log l;
        l.swap(log);
        return l;
    }
};

struct Fixture {
    FakeActions a;
    Controller c;
    explicit Fixture(Settings s = {}) : c(a, s) {}
    // Hotkey down at t=0 and the Start reply with id 7; the log is cleared.
    void started(uint32_t id = 7, bool modifier = true) {
        c.hotkeyPress(0, true, modifier);
        c.startReply(id);
        a.take();
    }
    // As started(), plus the daemon reports recording.
    void recording(uint32_t id = 7, bool modifier = true) {
        started(id, modifier);
        c.stateChanged(id, "recording");
        a.take();
    }
};

void hold_commits_final() {
    Fixture f;
    CHECK(f.c.hotkeyPress(0, true, true));
    CHECK_LOG(f.a.take(), "status Preparing", "timer 6000", "start");
    f.c.startReply(7);
    f.c.stateChanged(7, "starting");
    f.c.stateChanged(7, "recording");
    CHECK_LOG(f.a.take(), "untimer", "status Recording");
    f.c.partial(7, "あ");
    CHECK_LOG(f.a.take(), "partial あ");
    CHECK(f.c.hotkeyRelease(1000));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
    f.c.finalText(7, "あい");
    CHECK_LOG(f.a.take(), "untimer", "clear", "commit あい");
    f.c.stateChanged(7, "done");
    CHECK_LOG(f.a.take());
    CHECK(f.c.idle());
}

void tap_keeps_recording_until_next_press() {
    Fixture f;
    f.recording();
    CHECK(f.c.hotkeyRelease(100));
    CHECK_LOG(f.a.take(), "status RecordingTap");
    CHECK(f.c.hotkeyPress(2000, true, true));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
    CHECK(f.c.hotkeyRelease(2100));
    CHECK_LOG(f.a.take());
}

void tap_before_recording_state_shows_tap_status_later() {
    Fixture f;
    f.started();
    CHECK(f.c.hotkeyRelease(100));
    CHECK_LOG(f.a.take());
    f.c.stateChanged(7, "recording");
    CHECK_LOG(f.a.take(), "status RecordingTap");
}

void tap_toggle_off_release_stops() {
    Fixture f(Settings{false, 300, true});
    f.recording();
    CHECK(f.c.hotkeyRelease(100));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
}

void threshold_boundary() {
    {
        Fixture f;
        f.recording();
        f.c.hotkeyRelease(299);
        CHECK_LOG(f.a.take(), "status RecordingTap");
    }
    {
        Fixture f;
        f.recording();
        f.c.hotkeyRelease(300);
        CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
    }
}

void ineligible_press_and_release_pass() {
    Fixture f;
    CHECK(!f.c.hotkeyPress(0, false, true));
    CHECK(!f.c.hotkeyRelease(100));
    CHECK_LOG(f.a.take());
    CHECK(f.c.idle());
}

void repeats_are_swallowed() {
    Fixture f;
    f.recording(7, false);
    CHECK(f.c.hotkeyPress(30, true, false));
    CHECK(f.c.hotkeyPress(60, true, false));
    CHECK_LOG(f.a.take());
    CHECK(f.c.hotkeyRelease(1000));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
}

void repeats_of_passed_press_pass() {
    Fixture f;
    CHECK(!f.c.hotkeyPress(0, false, false));
    CHECK(!f.c.hotkeyPress(30, true, false));
    CHECK_LOG(f.a.take());
}

void lost_release_new_press_stops() {
    Fixture f;
    f.recording();
    CHECK(f.c.hotkeyPress(2000, true, true));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
    CHECK(f.c.hotkeyRelease(2100));
    CHECK_LOG(f.a.take());
}

void esc_cancels_and_is_swallowed() {
    Fixture f;
    f.recording();
    CHECK(f.c.escape());
    CHECK_LOG(f.a.take(), "cancel 7", "clear");
    CHECK(f.c.idle());
    f.c.partial(7, "x");
    f.c.stateChanged(7, "cancelled");
    CHECK_LOG(f.a.take());
    CHECK(f.c.hotkeyRelease(1000));
    CHECK_LOG(f.a.take());
}

void esc_when_idle_passes() {
    Fixture f;
    CHECK(!f.c.escape());
    CHECK_LOG(f.a.take());
}

void other_key_cancels_while_recording() {
    Fixture f;
    f.recording();
    CHECK(!f.c.otherKeyPress());
    CHECK_LOG(f.a.take(), "cancel 7", "clear");
    CHECK(f.c.idle());
}

void other_key_while_finalizing_releases_display() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    CHECK(!f.c.otherKeyPress());
    CHECK_LOG(f.a.take(), "clear");
    CHECK(!f.c.otherKeyPress());
    CHECK_LOG(f.a.take());
    f.c.finalText(7, "x");
    CHECK_LOG(f.a.take(), "untimer", "commit x");
}

void esc_while_finalizing_cancels() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    CHECK(f.c.escape());
    CHECK_LOG(f.a.take(), "cancel 7", "untimer", "clear");
}

void focus_lost_cancels() {
    Fixture f;
    f.recording();
    f.c.focusLost();
    CHECK_LOG(f.a.take(), "cancel 7", "clear");
    CHECK(f.c.idle());
}

void focus_lost_while_finalizing_cancels() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    f.c.focusLost();
    CHECK_LOG(f.a.take(), "cancel 7", "untimer", "clear");
}

void signals_before_reply_are_replayed() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    f.c.stateChanged(7, "starting");
    f.c.stateChanged(7, "recording");
    f.c.partial(7, "a");
    CHECK_LOG(f.a.take());
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "status Recording", "partial a");
}

void buffered_signals_for_other_ids_cancel_the_stray_session() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    f.c.partial(3, "x");
    f.c.partial(3, "y");
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "cancel 3");
}

void buffer_is_capped() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    for (int i = 0; i < 100; ++i) f.c.partial(7, std::to_string(i));
    f.c.startReply(7);
    CHECK(f.a.take().size() == Controller::kMaxBuffered + 1);
}

void release_before_reply_stops_after_reply() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    CHECK(f.c.hotkeyRelease(1000));
    CHECK_LOG(f.a.take());
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "status Finalizing", "timer 10000", "stop 7");
}

void esc_before_reply_cancels_after_reply() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    CHECK(f.c.escape());
    CHECK_LOG(f.a.take(), "clear");
    f.c.stateChanged(7, "recording");
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "cancel 7");
    CHECK(f.c.idle());
}

void start_errors_map_to_notices() {
    struct Case {
        CallError kind;
        const char *expected;
    } cases[] = {
        {CallError::NoDaemon, "notify NoDaemon"},
        {CallError::NoModel, "notify NoModel msg"},
        {CallError::Busy, "notify Busy"},
        {CallError::Timeout, "notify NoResponse"},
        {CallError::Other, "notify Failed msg"},
    };
    for (auto &k : cases) {
        Fixture f;
        f.c.hotkeyPress(0, true, true);
        f.a.take();
        f.c.startFailed(k.kind, "msg");
        CHECK_LOG(f.a.take(), "untimer", "clear", k.expected);
        CHECK(f.c.idle());
        CHECK(f.c.hotkeyRelease(1000));
        CHECK_LOG(f.a.take());
    }
}

void start_failure_after_user_cancel_is_quiet() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.c.escape();
    f.a.take();
    f.c.startFailed(CallError::NoDaemon, "");
    CHECK_LOG(f.a.take(), "untimer");
    CHECK(f.c.idle());
}

void empty_final_commits_nothing() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    f.c.finalText(7, "");
    CHECK_LOG(f.a.take(), "untimer", "clear");
    CHECK(f.c.idle());
}

void error_signal_notifies() {
    Fixture f;
    f.recording();
    f.c.error(7, "audio", "no mic");
    CHECK_LOG(f.a.take(), "clear", "notify AudioError no mic");
    f.c.stateChanged(7, "error");
    CHECK_LOG(f.a.take());
    Fixture g;
    g.recording();
    g.c.error(7, "model", "m");
    CHECK_LOG(g.a.take(), "clear", "notify ModelError m");
    Fixture h;
    h.recording();
    h.c.error(7, "internal", "i");
    CHECK_LOG(h.a.take(), "clear", "notify InternalError i");
}

void daemon_left_notifies() {
    Fixture f;
    f.recording();
    f.c.daemonLeft();
    CHECK_LOG(f.a.take(), "clear", "notify DaemonLeft");
    CHECK(f.c.idle());
    f.c.daemonLeft();
    CHECK_LOG(f.a.take());
}

void finalize_watchdog_cancels() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    f.c.timerFired();
    CHECK_LOG(f.a.take(), "cancel 7", "clear", "notify NoResponse");
    CHECK(f.c.idle());
}

void daemon_ends_session_while_key_down() {
    Fixture f;
    f.recording();
    f.c.stateChanged(7, "finalizing");
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000");
    f.c.finalText(7, "t");
    CHECK_LOG(f.a.take(), "untimer", "clear", "commit t");
    CHECK(f.c.hotkeyRelease(130000));
    CHECK_LOG(f.a.take());
}

void cancelled_state_ends_without_commit() {
    Fixture f;
    f.recording();
    f.c.stateChanged(7, "cancelled");
    CHECK_LOG(f.a.take(), "clear");
    CHECK(f.c.idle());
}

void done_without_final_clears() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    f.c.stateChanged(7, "done");
    CHECK_LOG(f.a.take(), "untimer", "clear");
    CHECK(f.c.idle());
}

void stop_failure_notifies() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    f.c.stopFailed(7, CallError::Other, "boom");
    CHECK_LOG(f.a.take(), "untimer", "clear", "notify Failed boom");
    CHECK(f.c.idle());
    Fixture g;
    g.recording();
    g.c.hotkeyRelease(1000);
    g.a.take();
    g.c.stopFailed(7, CallError::Timeout, "");
    CHECK_LOG(g.a.take(), "untimer", "clear", "notify NoResponse");
}

void stop_failure_for_old_session_is_ignored() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.c.finalText(7, "x");
    f.a.take();
    f.c.stopFailed(7, CallError::Other, "NoSuchSession");
    CHECK_LOG(f.a.take());
}

void partial_hidden_when_setting_off() {
    Fixture f(Settings{true, 300, false});
    f.recording();
    f.c.partial(7, "a");
    CHECK_LOG(f.a.take());
}

void reply_when_idle_cancels_stray_session() {
    Fixture f;
    f.c.startReply(9);
    CHECK_LOG(f.a.take(), "cancel 9");
}

void daemon_left_while_starting_then_start_failure() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    f.c.daemonLeft();
    CHECK_LOG(f.a.take(), "untimer", "clear", "notify DaemonLeft");
    f.c.startFailed(CallError::NoDaemon, "");
    CHECK_LOG(f.a.take());
}

void new_session_after_finish() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.c.finalText(7, "x");
    f.a.take();
    CHECK(f.c.hotkeyPress(5000, true, true));
    CHECK_LOG(f.a.take(), "status Preparing", "timer 6000", "start");
    f.c.startReply(8);
    f.c.stateChanged(7, "done");
    f.c.partial(8, "new");
    CHECK_LOG(f.a.take(), "untimer", "partial new");
}

void press_while_finalizing_is_swallowed() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.a.take();
    CHECK(f.c.hotkeyPress(1200, true, true));
    CHECK(f.c.hotkeyRelease(1300));
    CHECK_LOG(f.a.take());
}

void call_error_names() {
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.ServiceUnknown") == CallError::NoDaemon);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.NameHasNoOwner") == CallError::NoDaemon);
    CHECK(callErrorFromName("org.nemui.KoeInput.Error.NoModel") == CallError::NoModel);
    CHECK(callErrorFromName("org.nemui.KoeInput.Error.Busy") == CallError::Busy);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.NoReply") == CallError::Timeout);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.Timeout") == CallError::Timeout);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.TimedOut") == CallError::Timeout);
    CHECK(callErrorFromName("org.nemui.KoeInput.Error.NoSuchSession") == CallError::Other);
    CHECK(callErrorFromName("") == CallError::Other);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.Spawn.ExecFailed") == CallError::NoDaemon);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.Spawn.ServiceNotFound") == CallError::NoDaemon);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.Spawn.ChildExited") == CallError::NoDaemon);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.Spawn") == CallError::Other);
    CHECK(callErrorFromName("org.freedesktop.DBus.Error.SpawnFailed") == CallError::Other);
}

void first_repeat_of_plain_key_is_a_repeat() {
    Fixture f;
    f.recording(7, false);
    CHECK(f.c.hotkeyPress(600, true, false));
    CHECK(f.c.hotkeyPress(630, true, false));
    CHECK_LOG(f.a.take());
    CHECK(f.c.hotkeyRelease(2000));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
}

void plain_key_lost_release_after_long_gap() {
    Fixture f;
    f.recording(7, false);
    CHECK(f.c.hotkeyPress(1500, true, false));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
}

void repeat_gap_is_inclusive() {
    Fixture f;
    f.recording(7, false);
    CHECK(f.c.hotkeyPress(1000, true, false));
    CHECK_LOG(f.a.take());
}

void modifier_press_while_down_is_new_press() {
    Fixture f;
    f.recording();
    CHECK(f.c.hotkeyPress(100, true, true));
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7");
}

void other_key_in_starting_cancels() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    CHECK(!f.c.otherKeyPress());
    CHECK_LOG(f.a.take(), "clear");
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "cancel 7");
    CHECK(f.c.idle());
}

void other_key_in_starting_with_pending_stop_releases_display() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    CHECK(f.c.hotkeyRelease(1000));
    CHECK_LOG(f.a.take());
    CHECK(!f.c.otherKeyPress());
    CHECK_LOG(f.a.take(), "clear");
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "timer 10000", "stop 7");
    f.c.finalText(7, "x");
    CHECK_LOG(f.a.take(), "untimer", "commit x");
}

void focus_lost_in_starting() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    f.c.focusLost();
    CHECK_LOG(f.a.take(), "clear");
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "cancel 7");
    CHECK(f.c.idle());
}

void buffered_final_and_done_commit_once() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    f.c.stateChanged(7, "recording");
    f.c.finalText(7, "x");
    f.c.stateChanged(7, "done");
    CHECK_LOG(f.a.take());
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "status Recording", "clear", "commit x");
    CHECK(f.c.idle());
}

void buffered_finalizing_with_pending_stop() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    CHECK(f.c.hotkeyRelease(1000));
    f.c.stateChanged(7, "finalizing");
    CHECK_LOG(f.a.take());
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "status Finalizing", "timer 10000");
}

void second_press_in_starting_after_tap() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    CHECK(f.c.hotkeyRelease(100));
    CHECK(f.c.hotkeyPress(2000, true, true));
    CHECK_LOG(f.a.take());
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "untimer", "status Finalizing", "timer 10000", "stop 7");
}

void stale_timer_is_ignored() {
    Fixture f;
    f.recording();
    f.c.timerFired();
    CHECK_LOG(f.a.take());
}

void unknown_error_code_is_internal() {
    Fixture f;
    f.recording();
    f.c.error(7, "weird", "m");
    CHECK_LOG(f.a.take(), "clear", "notify InternalError m");
}

void full_buffer_keeps_final() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    for (int i = 0; i < 100; ++i) f.c.partial(7, std::to_string(i));
    f.c.finalText(7, "x");
    f.c.startReply(7);
    const Log l = f.a.take();
    CHECK(l.size() == 66);
    CHECK(l.size() < 2 || Log(l.end() - 2, l.end()) == Log({"clear", "commit x"}));
}

void start_watchdog_ends_starting() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    f.c.timerFired();
    CHECK_LOG(f.a.take(), "clear", "notify NoResponse");
    CHECK(f.c.idle());
    f.c.startReply(7);
    CHECK_LOG(f.a.take(), "cancel 7");
    CHECK(f.c.hotkeyRelease(1000));
    CHECK_LOG(f.a.take());
}

void start_watchdog_after_user_cancel_is_quiet() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    CHECK(f.c.escape());
    CHECK_LOG(f.a.take(), "clear");
    f.c.timerFired();
    CHECK_LOG(f.a.take());
    CHECK(f.c.idle());
}

void reply_stops_start_watchdog() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.a.take();
    f.c.startReply(7);
    const Log l = f.a.take();
    CHECK(!l.empty() && l.front() == "untimer");
    f.c.timerFired();
    CHECK_LOG(f.a.take());
    CHECK(!f.c.idle());
}

void stray_session_is_cancelled_once() {
    Fixture f;
    f.c.stateChanged(9, "recording");
    CHECK_LOG(f.a.take(), "cancel 9");
    f.c.partial(9, "x");
    CHECK_LOG(f.a.take());
    f.c.stateChanged(9, "cancelled");
    CHECK_LOG(f.a.take());
    CHECK(f.c.idle());
}

void stray_starting_and_partial_are_cancelled() {
    {
        Fixture f;
        f.c.stateChanged(9, "starting");
        CHECK_LOG(f.a.take(), "cancel 9");
    }
    {
        Fixture f;
        f.c.partial(9, "x");
        CHECK_LOG(f.a.take(), "cancel 9");
    }
    {
        Fixture f;
        f.c.stateChanged(9, "finalizing");
        f.c.stateChanged(9, "done");
        f.c.finalText(9, "x");
        f.c.error(9, "internal", "m");
        CHECK_LOG(f.a.take());
    }
}

void stray_session_during_own_session_is_cancelled() {
    Fixture f;
    f.recording();
    f.c.partial(3, "x");
    CHECK_LOG(f.a.take(), "cancel 3");
    f.c.partial(7, "a");
    CHECK_LOG(f.a.take(), "partial a");
    CHECK(!f.c.idle());
}

void own_cancel_is_not_repeated() {
    Fixture f;
    f.recording();
    CHECK(f.c.escape());
    CHECK_LOG(f.a.take(), "cancel 7", "clear");
    f.c.partial(7, "x");
    CHECK_LOG(f.a.take());
    f.c.stateChanged(7, "recording");
    CHECK_LOG(f.a.take());
}

void late_reply_after_stray_signal_is_cancelled_once() {
    Fixture f;
    f.c.hotkeyPress(0, true, true);
    f.c.timerFired();
    f.a.take();
    f.c.stateChanged(7, "starting");
    CHECK_LOG(f.a.take(), "cancel 7");
    f.c.startReply(7);
    CHECK_LOG(f.a.take());
}

// Session ids restart at 1 in every daemon process, so an id cancelled under
// the previous daemon can belong to a new stray session.
void daemon_left_resets_cancelled_id() {
    Fixture f;
    f.recording();
    CHECK(f.c.escape());
    CHECK_LOG(f.a.take(), "cancel 7", "clear");
    f.c.daemonLeft();
    CHECK_LOG(f.a.take());
    f.c.stateChanged(7, "recording");
    CHECK_LOG(f.a.take(), "cancel 7");
}

void finished_session_signals_do_not_cancel() {
    Fixture f;
    f.recording();
    f.c.hotkeyRelease(1000);
    f.c.finalText(7, "x");
    CHECK_LOG(f.a.take(), "status Finalizing", "timer 10000", "stop 7", "untimer", "clear", "commit x");
    f.c.stateChanged(7, "done");
    CHECK_LOG(f.a.take());
}

} // namespace

int main() {
    struct {
        const char *name;
        void (*fn)();
    } tests[] = {
#define T(name) {#name, name}
        T(hold_commits_final),
        T(tap_keeps_recording_until_next_press),
        T(tap_before_recording_state_shows_tap_status_later),
        T(tap_toggle_off_release_stops),
        T(threshold_boundary),
        T(ineligible_press_and_release_pass),
        T(repeats_are_swallowed),
        T(repeats_of_passed_press_pass),
        T(lost_release_new_press_stops),
        T(esc_cancels_and_is_swallowed),
        T(esc_when_idle_passes),
        T(other_key_cancels_while_recording),
        T(other_key_while_finalizing_releases_display),
        T(esc_while_finalizing_cancels),
        T(focus_lost_cancels),
        T(focus_lost_while_finalizing_cancels),
        T(signals_before_reply_are_replayed),
        T(buffered_signals_for_other_ids_cancel_the_stray_session),
        T(buffer_is_capped),
        T(release_before_reply_stops_after_reply),
        T(esc_before_reply_cancels_after_reply),
        T(start_errors_map_to_notices),
        T(start_failure_after_user_cancel_is_quiet),
        T(empty_final_commits_nothing),
        T(error_signal_notifies),
        T(daemon_left_notifies),
        T(finalize_watchdog_cancels),
        T(daemon_ends_session_while_key_down),
        T(cancelled_state_ends_without_commit),
        T(done_without_final_clears),
        T(stop_failure_notifies),
        T(stop_failure_for_old_session_is_ignored),
        T(partial_hidden_when_setting_off),
        T(reply_when_idle_cancels_stray_session),
        T(daemon_left_while_starting_then_start_failure),
        T(new_session_after_finish),
        T(press_while_finalizing_is_swallowed),
        T(call_error_names),
        T(first_repeat_of_plain_key_is_a_repeat),
        T(plain_key_lost_release_after_long_gap),
        T(repeat_gap_is_inclusive),
        T(modifier_press_while_down_is_new_press),
        T(other_key_in_starting_cancels),
        T(other_key_in_starting_with_pending_stop_releases_display),
        T(focus_lost_in_starting),
        T(buffered_final_and_done_commit_once),
        T(buffered_finalizing_with_pending_stop),
        T(second_press_in_starting_after_tap),
        T(stale_timer_is_ignored),
        T(unknown_error_code_is_internal),
        T(full_buffer_keeps_final),
        T(start_watchdog_ends_starting),
        T(start_watchdog_after_user_cancel_is_quiet),
        T(reply_stops_start_watchdog),
        T(stray_session_is_cancelled_once),
        T(stray_starting_and_partial_are_cancelled),
        T(stray_session_during_own_session_is_cancelled),
        T(own_cancel_is_not_repeated),
        T(late_reply_after_stray_signal_is_cancelled_once),
        T(finished_session_signals_do_not_cancel),
        T(daemon_left_resets_cancelled_id),
#undef T
    };
    for (auto &t : tests) {
        int before = failures;
        t.fn();
        std::printf("%s %s\n", failures == before ? "ok  " : "FAIL", t.name);
    }
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
