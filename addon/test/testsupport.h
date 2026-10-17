#pragma once

#include <fcitx-utils/event.h>
#include <cerrno>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <memory>
#include <string>
#include <sys/wait.h>
#include <unistd.h>
#include <utility>
#include <vector>

extern char **environ;

namespace koeinput::test {

inline int failures = 0;

#define KI_CHECK(cond)                                                         \
    do {                                                                       \
        if (!(cond)) {                                                         \
            std::fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__,       \
                         __LINE__, #cond);                                     \
            ++::koeinput::test::failures;                                      \
        }                                                                      \
    } while (0)

// Runs steps one after another on an fcitx5 event loop. Each step waits
// delayMs after the previous one; `done` runs after the last step.
class Steps {
public:
    struct Step {
        uint64_t delayMs;
        std::string name;
        std::function<void()> fn;
    };

    Steps(fcitx::EventLoop &loop, std::function<void()> done)
        : loop_(loop), done_(std::move(done)) {}

    Steps &then(uint64_t delayMs, std::string name, std::function<void()> fn) {
        steps_.push_back({delayMs, std::move(name), std::move(fn)});
        return *this;
    }

    void start() { schedule(0); }

private:
    void schedule(size_t i) {
        if (i == steps_.size()) {
            pending_.reset();
            done_();
            return;
        }
        // Accuracy 1 us: sd-event turns accuracy 0 into its 250 ms default.
        pending_ = loop_.addTimeEvent(
            CLOCK_MONOTONIC, fcitx::now(CLOCK_MONOTONIC) + steps_[i].delayMs * 1000, 1,
            [this, i](fcitx::EventSourceTime *, uint64_t) {
                std::fprintf(stderr, "step: %s\n", steps_[i].name.c_str());
                steps_[i].fn();
                // Replacing pending_ here would destroy the running event source.
                deferred_ = loop_.addDeferEvent([this, i](fcitx::EventSource *) {
                    schedule(i + 1);
                    return true;
                });
                return true;
            });
    }

    fcitx::EventLoop &loop_;
    std::function<void()> done_;
    std::vector<Step> steps_;
    std::unique_ptr<fcitx::EventSourceTime> pending_;
    std::unique_ptr<fcitx::EventSource> deferred_;
};

// The M1 daemon built with test-hooks, on the private test bus.
class DaemonProcess {
public:
    static const char *binary() {
        const char *b = std::getenv("KOEINPUT_TEST_DAEMON");
        return (b && *b) ? b : nullptr;
    }

    explicit DaemonProcess(std::vector<std::pair<std::string, std::string>> extraEnv = {}) {
        char dirTemplate[] = "/tmp/koeinput-test-XXXXXX";
        dir_ = mkdtemp(dirTemplate);
        std::vector<std::string> env;
        for (char **e = environ; *e; ++e) env.emplace_back(*e);
        env.push_back("HOME=" + dir_);
        env.push_back("XDG_CONFIG_HOME=" + dir_ + "/config");
        env.push_back("XDG_DATA_HOME=" + dir_ + "/data");
        env.push_back("KOEINPUT_TEST_FAKE=1");
        for (auto &[k, v] : extraEnv) env.push_back(k + "=" + v);
        std::vector<char *> envp;
        for (auto &s : env) envp.push_back(s.data());
        envp.push_back(nullptr);
        std::string log = dir_ + "/stderr.log";
        pid_ = fork();
        if (pid_ == 0) {
            FILE *f = std::freopen(log.c_str(), "w", stderr);
            (void)f;
            char *argv[] = {const_cast<char *>(binary()), nullptr};
            execve(argv[0], argv, envp.data());
            _exit(127);
        }
        waitUntilReady();
    }

    ~DaemonProcess() { stop(); }

    void stop() {
        if (pid_ <= 0) return;
        ::kill(pid_, SIGTERM);
        int status = 0;
        waitpid(pid_, &status, 0);
        pid_ = -1;
        // Wait until the bus has released the name, so the next daemon can take it.
        for (int i = 0; i < 100 && ownerPresent(); ++i) usleep(20000);
    }

private:
    static bool ownerPresent() {
        return std::system("dbus-send --session --print-reply --dest=org.freedesktop.DBus "
                           "/org/freedesktop/DBus org.freedesktop.DBus.GetNameOwner "
                           "string:org.nemui.KoeInput >/dev/null 2>&1") == 0;
    }

    void waitUntilReady() {
        const char *cmd =
            "dbus-send --session --print-reply --dest=org.nemui.KoeInput /org/nemui/KoeInput "
            "org.freedesktop.DBus.Properties.Get string:org.nemui.KoeInput.Recognizer1 "
            "string:Ready 2>/dev/null";
        for (int i = 0; i < 200; ++i) {
            if (FILE *p = popen(cmd, "r")) {
                char buf[512];
                std::string out;
                while (std::fgets(buf, sizeof buf, p)) out += buf;
                pclose(p);
                // A model that fails to load (KOEINPUT_TEST_NO_MODEL) never becomes ready.
                if (out.find("boolean true") != std::string::npos ||
                    (out.find("boolean false") != std::string::npos && notReadyOk_)) {
                    return;
                }
            }
            usleep(50000);
        }
        std::fprintf(stderr, "daemon did not become ready; log in %s\n", dir_.c_str());
        std::abort();
    }

public:
    // For a daemon that is expected to stay not ready (no model).
    static DaemonProcess notReady(std::vector<std::pair<std::string, std::string>> extraEnv) {
        notReadyOk_ = true;
        DaemonProcess d(std::move(extraEnv));
        notReadyOk_ = false;
        return d;
    }
    DaemonProcess(DaemonProcess &&o) noexcept : pid_(o.pid_), dir_(std::move(o.dir_)) { o.pid_ = -1; }

private:
    static inline bool notReadyOk_ = false;
    pid_t pid_ = -1;
    std::string dir_;
};

} // namespace koeinput::test
