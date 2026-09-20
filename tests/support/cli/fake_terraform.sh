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
    exit 2
    ;;
  apply)
    plan_path=''
    for argument in "$@"; do
      case "$argument" in
        *.tfplan) plan_path=$argument ;;
      esac
    done
    test -n "$plan_path"
    test -f "$plan_path"
    printf '%s\n' "$$" > "$TERRACOTTA_FAKE_PID_PATH"
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = apply_interrupt ]; then
      printf 'Applying saved plan...\n'
      exec python3 -c 'import signal,sys,time; signal.signal(signal.SIGINT, lambda *_: (print("Stopping apply", flush=True), time.sleep(1), sys.exit(130))); time.sleep(30)'
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = apply_failure ]; then
      sleep 1
      printf 'Error: synthetic apply failure\n' >&2
      printf 'Changes may already be applied.\n' >&2
      exit 1
    fi
    printf 'Applying saved plan...\n'
    sleep 1
    printf 'Apply complete! Resources: 1 added, 1 changed, 0 destroyed.\n'
    printf 'Outputs:\nendpoint = "https://example.test"\n'
    exit 0
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
