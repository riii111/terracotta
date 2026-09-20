#!/bin/sh
set -eu

printf '%s|%s\n' "$PWD" "$*" >> "$TERRACOTTA_FAKE_INVOCATIONS"

case "$1" in
  init)
    printf 'Initializing the backend...\n'
    printf 'Initializing provider plugins...\n' >&2
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = init_failure ]; then
      printf 'synthetic init failure\n' >&2
      exit 1
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = diagnostic_success ]; then
      printf '╷\n│ Warning: synthetic init warning\n│\n│ fake Terraform initialized with a warning\n╵\n' >&2
    fi
    ;;
  workspace)
    printf 'default\n'
    ;;
  plan)
    plan_path=''
    for argument in "$@"; do
      case "$argument" in
        -out=*) plan_path=${argument#-out=} ;;
      esac
    done
    printf '%s\n' "$plan_path" > "$TERRACOTTA_FAKE_PLAN_PATH"
    printf '%s\n' "$$" > "$TERRACOTTA_FAKE_PID_PATH"
    : > "$plan_path"
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = interrupt ]; then
      exec python3 -c 'import signal,sys,time; signal.signal(signal.SIGINT, lambda *_: (time.sleep(1), sys.exit(130))); time.sleep(30)'
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = failure ]; then
      printf '%s\n' '{"type":"diagnostic","diagnostic":{"severity":"error","summary":"synthetic plan failure","detail":"fake Terraform failed"}}'
      exit 1
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = diagnostic_success ]; then
      printf '%s\n' '{"type":"diagnostic","diagnostic":{"severity":"warning","summary":"synthetic plan warning","detail":"fake Terraform completed with a warning"}}'
    fi
    printf '%s\n' '{"type":"planned_change","change":{"resource":{"addr":"terraform_data.api"}}}'
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = no_changes ]; then
      exit 0
    fi
    exit 2
    ;;
  show)
    if [ "$2" = -json ]; then
      cat "$TERRACOTTA_FAKE_SHOW_JSON"
    else
      cat "$TERRACOTTA_FAKE_SHOW_TEXT"
    fi
    ;;
  *)
    exit 2
    ;;
esac
