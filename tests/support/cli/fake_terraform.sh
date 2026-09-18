#!/bin/sh
set -eu

case "$1" in
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
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = config_interrupt ]; then
      : > "$TERRACOTTA_FAKE_PLAN_DONE_PATH"
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = interrupt ]; then
      exec /bin/sleep 30
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = failure ]; then
      printf '%s\n' '{"type":"diagnostic","diagnostic":{"severity":"error","summary":"synthetic plan failure","detail":"fake Terraform failed"}}'
      exit 1
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = diagnostic_success ]; then
      printf '%s\n' '{"type":"diagnostic","diagnostic":{"severity":"warning","summary":"synthetic plan warning","detail":"fake Terraform completed with a warning"}}'
    fi
    printf '%s\n' '{"type":"planned_change","change":{"resource":{"addr":"terraform_data.api"}}}'
    ;;
  show)
    cat "$TERRACOTTA_FAKE_SHOW_JSON"
    ;;
  *)
    exit 2
    ;;
esac
