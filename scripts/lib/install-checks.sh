# The install test that scripts/test-deb, scripts/test-rpm and scripts/test-arch
# run in a fresh container of the distribution. The test script mounts this file
# and a format file next to it, format.sh, which sets what differs between the
# package formats:
#   PKGS              the two package files to install
#   DAEMON            the installed path of koeinput-daemon
#   ADDON_DIR         the directory of libkoeinput.so
#   COPYRIGHT         the copyright file of koeinput, with the notices of the crates
#   NOTICES           a pattern for the installed ONNX Runtime notices file
#   KOEINPUT_DOC_FILES, ADDON_DOC_FILES
#                     more files that each package must install (documentation and
#                     licenses)
#   REMOVED           more paths that must be gone after the removal
#   REMOVE_LABEL      the removal command, for messages
#   TIMEOUT           seconds for the whole test (default 900)
# and these functions:
#   prepare_image     undoes what the image leaves out (optional)
#   first_install     installs PKGS the first time (default: install_packages)
#   install_packages  installs PKGS
#   install_tools     installs man and the D-Bus tools for the test
#   show_versions     prints the versions of the packages and of fcitx5 and dbus
#   owner FILE        prints the name of the package that installs FILE
#   list_files        prints the files of both packages
#   check_package_extra  more checks after each install (optional)
#   check_files_extra    more checks of the installed files (optional)
#   remove_packages   removes both packages
#   is_installed PKG  succeeds when PKG is installed
#
# The script runs as root in the container ("root") and calls itself as an
# ordinary user for the checks that do not need root ("user"), and once more
# inside each private session bus ("status-bus", "fcitx5-bus").
set -eu
self=$0
step() { printf '\n=== %s\n' "$*"; }
ok() { echo "ok: $*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
warn() {
  echo "WARNING: $*" >&2
  echo "  $*" >>/tmp/warnings
}
# Prints lines from a log with a prefix, so they stand apart in the output.
show() { sed "s/\x1b\[[0-9;]*m//g; s|^|  $1: |" "$2" 2>/dev/null || true; }
wait_for() {
  i=0
  while ! grep -q "$1" "$2" 2>/dev/null; do
    [ $i -lt $(($3 * 2)) ] || return 1
    sleep 0.5
    i=$((i + 1))
  done
}

KOEINPUT_DOC_FILES= ADDON_DOC_FILES= REMOVED= TIMEOUT=900
prepare_image() { :; }
first_install() { install_packages; }
check_package_extra() { :; }
check_files_extra() { :; }
. "${self%/*}/format.sh"
: "${PKGS:?}" "${DAEMON:?}" "${ADDON_DIR:?}" "${COPYRIGHT:?}" "${NOTICES:?}" "${REMOVE_LABEL:?}"

KOEINPUT_FILES="/usr/bin/koeinput $DAEMON /usr/lib/systemd/user/koeinput.service
  /usr/share/dbus-1/services/org.nemui.KoeInput.service /usr/share/koeinput/models.toml
  /usr/share/man/man1/koeinput.1.gz $COPYRIGHT $KOEINPUT_DOC_FILES"
ADDON_FILES="$ADDON_DIR/libkoeinput.so /usr/share/fcitx5/addon/koeinput.conf
  /usr/share/locale/ja/LC_MESSAGES/fcitx5-koeinput.mo $ADDON_DOC_FILES"

check_installed() {
  for f in $KOEINPUT_FILES; do
    [ -f "$f" ] && [ "$(owner "$f")" = koeinput ] || fail "$f is not installed by koeinput"
  done
  for f in $ADDON_FILES; do
    [ -f "$f" ] && [ "$(owner "$f")" = fcitx5-koeinput ] || fail "$f is not installed by fcitx5-koeinput"
  done
  [ -x /usr/bin/koeinput ] && [ -x "$DAEMON" ] || fail "the binaries are not executable"
  check_package_extra
}

root_main() {
  prepare_image
  step "install $PKGS"
  first_install
  # Test tools only, installed after the packages so their own dependencies are tested.
  install_tools
  show_versions

  step "installed files"
  check_installed
  grep -qx "Exec=$DAEMON" /usr/share/dbus-1/services/org.nemui.KoeInput.service ||
    fail "the D-Bus service file does not start $DAEMON"
  grep -qx "ExecStart=$DAEMON" /usr/lib/systemd/user/koeinput.service ||
    fail "the user unit does not start $DAEMON"
  # The daemon starts by D-Bus activation only.
  enabled=$(find /etc/systemd/user /usr/lib/systemd/user -path '*.wants/koeinput.service' 2>/dev/null || true)
  [ -z "$enabled" ] || fail "the user unit is enabled: $enabled"
  grep -q '^Files: vendor/' "$COPYRIGHT" || fail "$COPYRIGHT has no paragraphs for the vendored crates"
  # NOTICES is a pattern; the first match is enough.
  set -- $NOTICES
  [ -f "$1" ] || fail "the ONNX Runtime notices ($NOTICES) are not installed"
  check_files_extra
  list_files | while read -r f; do [ ! -f "$f" ] || echo "  $f"; done
  ok "files in place"

  useradd -m -s /bin/sh tester
  install -d -o tester -g tester -m 700 /tmp/runtime-tester
  runuser -u tester -- env -i HOME=/home/tester PATH=/usr/bin:/bin LANG=C.UTF-8 \
    XDG_RUNTIME_DIR=/tmp/runtime-tester VERSION="$VERSION" UPSTREAM="$UPSTREAM" sh "$self" user

  step "remove"
  remove_packages
  for f in $KOEINPUT_FILES $ADDON_FILES "${DAEMON%/*}" /usr/share/koeinput $REMOVED; do
    [ ! -e "$f" ] || fail "$f is still there after $REMOVE_LABEL"
  done
  for pkg in koeinput fcitx5-koeinput; do
    ! is_installed "$pkg" || fail "$pkg is still installed"
  done
  ok "files removed"

  step "install again"
  install_packages
  check_installed
  [ "$(koeinput --version)" = "koeinput $UPSTREAM" ] || fail "koeinput --version after the second install"
  ok "installed again"

  if [ -e /tmp/warnings ]; then
    printf '\nPASSED WITH WARNINGS:\n'
    cat /tmp/warnings
  else
    printf '\nPASSED: all checks\n'
  fi
}

user_main() {
  cd "$HOME"
  step "koeinput command (as user $(id -un))"
  v=$(koeinput --version)
  echo "  $v"
  [ "$v" = "koeinput $UPSTREAM" ] || fail "koeinput --version printed '$v', not 'koeinput $UPSTREAM'"
  koeinput model list >models.txt
  show "model list" models.txt
  grep -q '^parakeet-ja .*not installed, default' models.txt || fail "koeinput model list does not show parakeet-ja"
  page=$(man -w koeinput)
  [ "$page" = /usr/share/man/man1/koeinput.1.gz ] || fail "man -w koeinput gave '$page'"
  MANWIDTH=80 man -P cat koeinput >man.txt 2>man.err || fail "man koeinput failed: $(cat man.err)"
  [ ! -s man.err ] || fail "man koeinput printed warnings: $(cat man.err)"
  head -3 man.txt | sed 's|^|  man: |'
  ok "--version, model list and man page"

  step "D-Bus activation by koeinput status"
  # No systemd in the container, so the bus starts the daemon through the Exec= line.
  st=0
  timeout 30 dbus-run-session -- sh "$self" status-bus 2>status-bus.log || st=$?
  show status status.txt
  show bus status-bus.log
  [ $st -eq 0 ] || fail "koeinput status under dbus-run-session failed (status $st)"
  grep -qx 'ready: no' status.txt || fail "koeinput status did not report 'ready: no'"
  grep -qx "version: $UPSTREAM" status.txt || fail "koeinput status did not report version $UPSTREAM"
  grep -q 'no model installed; run `koeinput model install parakeet-ja`' status-bus.log ||
    fail "the daemon did not log the install hint"
  ok "the bus started the daemon; not ready without a model, with the install hint"

  step "fcitx5 loads the addon (headless)"
  st=0
  timeout 40 dbus-run-session -- sh "$self" fcitx5-bus 2>fcitx5.log || st=$?
  grep -i 'koeinput\|error\|fatal\|signal' fcitx5.log | show fcitx5 -
  alive=$(cat fcitx5-alive 2>/dev/null || echo unknown)
  stopped=$(cat fcitx5-stopped 2>/dev/null || echo unknown)
  code=$(cat fcitx5-status 2>/dev/null || echo 0)
  echo "  fcitx5 running after 5 s: $alive; stopped by: $stopped; exit status: $code; dbus-run-session: $st"
  if grep -q 'Loaded addon koeinput' fcitx5.log; then
    [ "$alive" = yes ] || fail "fcitx5 stopped within 5 s of loading the addon (exit status $code)"
    if [ "$stopped" = exit-request ]; then
      [ "$code" -eq 0 ] || fail "fcitx5 exited with status $code after loading the addon"
    else
      warn "fcitx5 did not exit on fcitx5-remote -e and was killed"
    fi
    grep -q "Activating service name='org.nemui.KoeInput' requested by .*comm=\"fcitx5" fcitx5.log ||
      fail "fcitx5 did not ask the bus to start the daemon"
    grep -q 'boolean true' activation.txt || fail "the daemon was not running (NameHasOwner: $(cat activation.txt))"
    ! grep -q 'Cannot start the KoeInput daemon' fcitx5.log || fail "the addon could not start the daemon"
    grep -q 'no model installed' fcitx5.log || fail "the daemon started by the addon did not report the missing model"
    ok "addon loaded, and it started the daemon through D-Bus activation"
  elif grep -v 'Override Enabled Addons' fcitx5.log | grep -q koeinput || [ "$code" -ge 128 ] ||
    { [ "$alive" = no ] && [ "$code" -ne 0 ]; }; then
    fail "fcitx5 did not finish loading the addon"
  else
    warn "fcitx5 could not run headless here, so the addon load was not checked (see the fcitx5 lines above)"
  fi
}

# Runs inside dbus-run-session; stderr (with the daemon's log) goes to the caller's file.
status_bus() {
  timeout 10 koeinput status >status.txt
  # The status reply can come before the daemon has looked for the model.
  wait_for 'no model installed' status-bus.log 10 || true
}

fcitx5_bus() {
  # The subshell writes fcitx5's exit status when it ends; "kill -0" cannot tell
  # a running process from one that has exited but not been waited for.
  (
    fcitx5 --disable=all --enable=keyboard,dbus,koeinput --verbose='*=5' &
    echo $! >fcitx5.pid
    st=0
    wait $! || st=$?
    echo $st >fcitx5-status
  ) &
  sleep 5
  if [ -e fcitx5-status ]; then echo no; else echo yes; fi >fcitx5-alive
  dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus.NameHasOwner string:org.nemui.KoeInput >activation.txt 2>&1 || true
  fcitx5-remote -e 2>/dev/null || true
  i=0
  while [ ! -e fcitx5-status ] && [ $i -lt 10 ]; do
    sleep 0.5
    i=$((i + 1))
  done
  if [ -e fcitx5-status ]; then
    echo exit-request >fcitx5-stopped
  else
    echo kill >fcitx5-stopped
    kill "$(cat fcitx5.pid)" 2>/dev/null || true
  fi
  wait
}

# PID 1 of the container. A shell as PID 1 ignores signals it has no trap for, so
# this passes them on. timeout ends a test that hangs; it cannot be PID 1 itself
# (coreutils 9.10 and later exit with status 125 there).
pid1() {
  timeout -k 10 "$TIMEOUT" sh "$self" root &
  pid=$!
  trap 'kill -TERM $pid 2>/dev/null' HUP INT TERM
  while :; do
    st=0
    wait $pid || st=$?
    kill -0 $pid 2>/dev/null || exit $st
  done
}

case "${1:-root}" in
  pid1) pid1 ;;
  root) root_main ;;
  user) user_main ;;
  status-bus) status_bus ;;
  fcitx5-bus) fcitx5_bus ;;
  *) fail "unknown mode $1" ;;
esac
