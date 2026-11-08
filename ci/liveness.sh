#!/usr/bin/env bash
# Copyright (c) 2026 Adequate Systems HK Limited
# Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.
# Desktop liveness on a CI runner.
#
# The built desktop app is started once with wgpu and once with tiny-skia.
# Each launch passes only if the app's process is still running after a fixed
# wait and its log holds no panic, which catches start-up failures such as a
# shader or surface error. The check does not look at what the window draws;
# the headless screenshots cover that. On Linux the wgpu launch uses software
# Vulkan (lavapipe), so it does not test a graphics card.
#
# Usage: ci/liveness.sh TARGET_BINARY
#   LIVENESS_SECONDS  how long each launch must survive; 20 by default.

# Neither -e nor -u: a launch that fails is recorded, and the next one still
# runs. (bash 3.2, which macOS ships, also reports an empty array as unbound
# under -u, and the wrap array is empty off Linux.)
set -o pipefail

app=$1
if [ -z "$app" ] || [ ! -x "$app" ]; then
  echo "usage: ci/liveness.sh TARGET_BINARY (no executable at '$app')" >&2
  exit 2
fi
seconds=${LIVENESS_SECONDS:-20}

# Each launch is a job in a process group of its own. The group is what the
# stop below signals, so the app and the X server under it stop together.
set -m

# On Linux the app runs under xvfb-run. `-a` picks a free display number. The
# screen is given explicitly, 24 bits deep, rather than left to xvfb-run's
# default.
linux=false
wrap=()
if [ "$(uname -s)" = Linux ]; then
  linux=true
  wrap=(xvfb-run -a -s "-screen 0 1280x1024x24")
fi

failed=0
for renderer in wgpu tiny-skia; do
  work=$(mktemp -d) || exit 2
  home=$work/home
  mkdir -p "$home"
  log=$work/app.log
  status=$work/status

  # ICED_BACKEND names the renderer. Left unset, iced tries wgpu and silently
  # falls back to tiny-skia (renderer/src/fallback.rs), so a wgpu launch could
  # pass on tiny-skia. Named, a wgpu that cannot start ends the app instead,
  # because the tiny-skia compositor refuses any name but its own
  # (tiny_skia/src/window/compositor.rs, in the pinned iced fork).
  env_set=(HOME="$home" XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config"
    XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" ICED_BACKEND="$renderer")
  # On Linux the wgpu launch takes Vulkan only, so on a runner with no graphics
  # card its adapter is lavapipe and not an OpenGL device. WGPU_BACKEND is read
  # by wgpu itself.
  if $linux && [ "$renderer" = wgpu ]; then
    env_set+=(WGPU_BACKEND=vulkan)
  fi

  echo "::group::desktop, renderer $renderer"
  ( env "${env_set[@]}" "${wrap[@]}" "$app" </dev/null >"$log" 2>&1; echo $? >"$status" ) &
  pid=$!
  # Not waited for, and not reported by bash as a terminated job when the stop
  # below ends it: the status file says how the launch ended.
  disown "$pid"
  sleep "$seconds"

  # The verdict is read before the stop, because the stop makes the app exit,
  # and that writes the status file too.
  exited=
  if [ -s "$status" ]; then exited=$(cat "$status"); fi
  # $pid is the subshell around env, xvfb-run and the app, so its being alive
  # does not show that the app started. The app's own process is looked for
  # in the launch's group by its exact name, which is what the kernel records
  # (at most 15 characters on Linux); the wrappers' command lines also hold the
  # app's path, so a match on the full command line would find them instead.
  running=false
  if pgrep -g "$pid" -x "$(basename "$app")" >/dev/null; then running=true; fi

  # TERM goes to the whole group. KILL is for whatever ignores TERM, so that the
  # step cannot hang on a launch.
  kill -TERM -- "-$pid" 2>/dev/null
  sleep 2
  kill -KILL -- "-$pid" 2>/dev/null

  echo "the log:"
  cat "$log"
  echo "::endgroup::"

  if [ -n "$exited" ]; then
    echo "::error::desktop, renderer $renderer: the app exited before $seconds s, with status $exited"
    failed=1
  elif ! $running; then
    echo "::error::desktop, renderer $renderer: after $seconds s the launch is running but the app is not"
    failed=1
  # A panic on another thread, such as the wallet's worker, does not end the
  # process, so the log is read as well as the process.
  elif grep -q panicked "$log"; then
    echo "::error::desktop, renderer $renderer: the log holds a panic"
    failed=1
  else
    echo "desktop, renderer $renderer: still running after $seconds s, no panic in the log"
  fi
  rm -rf "$work"
done
exit "$failed"
