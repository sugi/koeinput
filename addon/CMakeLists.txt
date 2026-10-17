cmake_minimum_required(VERSION 3.25)
project(fcitx5-koeinput VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
add_compile_options(-Wall -Wextra)

find_package(Fcitx5Core REQUIRED)
find_package(Fcitx5Utils REQUIRED)
find_package(Fcitx5Config REQUIRED)
find_package(Fcitx5Module REQUIRED COMPONENTS DBus Notifications TestFrontend)

set(KOEINPUT_GETTEXT_DOMAIN "fcitx5-koeinput")

add_library(koeinput MODULE src/koeinput.cpp)
target_link_libraries(koeinput PRIVATE Fcitx5::Core Fcitx5::Config Fcitx5::Utils
  Fcitx5::Module::DBus Fcitx5::Module::Notifications)
target_compile_definitions(koeinput PRIVATE
  FCITX_GETTEXT_DOMAIN="${KOEINPUT_GETTEXT_DOMAIN}"
  FCITX_INSTALL_LOCALEDIR="${FCITX_INSTALL_LOCALEDIR}")
set_target_properties(koeinput PROPERTIES PREFIX "lib" LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}")

configure_file(koeinput.conf.in "${CMAKE_BINARY_DIR}/testdata/addon/koeinput.conf" @ONLY)

install(TARGETS koeinput DESTINATION "${FCITX_INSTALL_ADDONDIR}")
install(FILES "${CMAKE_BINARY_DIR}/testdata/addon/koeinput.conf"
  DESTINATION "${FCITX_INSTALL_PKGDATADIR}/addon")

enable_testing()

# The system dbus descriptor depends on keyboard, which is built into the fcitx5
# binary and cannot load in a test Instance. This copy drops that dependency.
configure_file(test/dbus.conf "${CMAKE_BINARY_DIR}/testdata/addon/dbus.conf" COPYONLY)
add_executable(addon_test test/addon_test.cpp)
target_link_libraries(addon_test PRIVATE Fcitx5::Core Fcitx5::Utils Fcitx5::Module::TestFrontend)
target_compile_definitions(addon_test PRIVATE
  KOEINPUT_BINARY_DIR="${CMAKE_BINARY_DIR}"
  FCITX5_SYSTEM_ADDONDIR="${FCITX_INSTALL_ADDONDIR}"
  FCITX5_SYSTEM_PKGDATADIR="${FCITX_INSTALL_PKGDATADIR}")
add_dependencies(addon_test koeinput)
add_test(NAME addon COMMAND addon_test)
set_tests_properties(addon PROPERTIES TIMEOUT 120 SKIP_RETURN_CODE 77)
