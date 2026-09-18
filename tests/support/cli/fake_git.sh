#!/bin/sh
set -eu

arguments=" $* "
mode="${TERRACOTTA_FAKE_MODE:-success}"
if [ "$mode" = git_interrupt ] && printf '%s' "$arguments" | grep -q ' diff '; then
  printf '%s\n' "$$" > "$TERRACOTTA_FAKE_GIT_PID_PATH"
  while :; do :; done
fi
if [ "$mode" = config_interrupt ] && [ -f "$TERRACOTTA_FAKE_PLAN_DONE_PATH" ]; then
  case "$arguments" in
    *" ls-tree "*|*" show "*)
      printf '%s\n' "$$" > "$TERRACOTTA_FAKE_GIT_PID_PATH"
      while :; do :; done
      ;;
  esac
fi
exec "$TERRACOTTA_REAL_GIT" "$@"
