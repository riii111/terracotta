#!/bin/sh
set -eu

printf '%s|%s\n' "$PWD" "$*" >> "$TERRACOTTA_FAKE_INVOCATIONS"
printf 'TF_CLI_ARGS=%s\n' "${TF_CLI_ARGS-}" >> "$TERRACOTTA_FAKE_ENV_LOG"
printf 'TF_CLI_ARGS_plan=%s\n' "${TF_CLI_ARGS_plan-}" >> "$TERRACOTTA_FAKE_ENV_LOG"
printf 'TF_CLI_ARGS_apply=%s\n' "${TF_CLI_ARGS_apply-}" >> "$TERRACOTTA_FAKE_ENV_LOG"

case "$1" in
  version)
    printf '%s\n' '{"terraform_version":"1.9.0"}'
    ;;
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
    printf '%s\n' "${TERRACOTTA_FAKE_WORKSPACE:-default}"
    ;;
  plan)
    plan_path=''
    previous=''
    for argument in "$@"; do
      case "$argument" in
        -out=*) plan_path=${argument#-out=} ;;
        -out) previous=out ;;
        *)
          if [ "$previous" = out ]; then
            plan_path=$argument
            previous=''
          fi
          ;;
      esac
    done
    printf '%s\n' "$plan_path" > "$TERRACOTTA_FAKE_PLAN_PATH"
    : > "$plan_path"
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = interrupt ]; then
      exec python3 -c 'import os,signal,sys,time; signal.signal(signal.SIGINT, lambda *_: (open(os.environ["TERRACOTTA_FAKE_SIGNAL_LOG"], "a").write("SIGINT\n"), time.sleep(1), sys.exit(130))); open(os.environ["TERRACOTTA_FAKE_PID_PATH"], "w").write(f"{os.getpid()}\n"); time.sleep(30)'
    fi
    printf '%s\n' "$$" > "$TERRACOTTA_FAKE_PID_PATH"
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = failure ]; then
      printf '%s\n' '{"type":"diagnostic","diagnostic":{"severity":"error","summary":"synthetic plan failure","detail":"fake Terraform failed"}}'
      exit 1
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = diagnostic_success ]; then
      printf '%s\n' '{"type":"diagnostic","diagnostic":{"severity":"warning","summary":"synthetic plan warning","detail":"fake Terraform completed with a warning"}}'
    fi
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = no_changes ]; then
      exit 0
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
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = apply_interrupt ]; then
      exec python3 -c 'import os,signal,sys,time; signal.signal(signal.SIGINT, lambda *_: (print("Stopping apply", flush=True), time.sleep(1), sys.exit(130))); open(os.environ["TERRACOTTA_FAKE_PID_PATH"], "w").write(f"{os.getpid()}\n"); print("{\"type\":\"apply_start\",\"@message\":\"Applying saved plan...\",\"hook\":{\"resource\":{\"addr\":\"terraform_data.api\"}}}", flush=True); time.sleep(30)'
    fi
    printf '%s\n' "$$" > "$TERRACOTTA_FAKE_PID_PATH"
    if [ "${TERRACOTTA_FAKE_MODE:-success}" = apply_failure ]; then
      sleep 1
      printf '%s\n' '{"type":"apply_start","@message":"Applying saved plan...","hook":{"resource":{"addr":"terraform_data.api"}}}'
      printf '%s\n' '{"type":"diagnostic","@level":"error","diagnostic":{"severity":"error","summary":"synthetic apply failure","detail":"Changes may already be applied. must-not-be-logged","address":"terraform_data.api"}}'
      printf '%s\n' '{"type":"apply_errored","@message":"Apply failed","hook":{"resource":{"addr":"terraform_data.api"}}}'
      exit 1
    fi
    printf '%s\n' '{"type":"apply_start","@message":"Applying saved plan...","hook":{"resource":{"addr":"terraform_data.api"}}}'
    sleep 1
    printf '%s\n' '{"type":"apply_progress","@message":"terraform_data.api: Applying must-not-be-logged","hook":{"resource":{"addr":"terraform_data.api"}}}'
    printf '%s\n' '{"type":"apply_complete","@message":"terraform_data.api: Creation complete","hook":{"resource":{"addr":"terraform_data.api"}}}'
    printf '%s\n' '{"type":"change_summary","@message":"Apply complete! Resources: 1 added, 1 changed, 0 destroyed.","changes":{"add":1,"change":1,"remove":0,"operation":"apply"}}'
    printf '%s\n' '{"type":"outputs","@message":"endpoint = \"https://example.test\""}'
    exit 0
    ;;
  show)
    if [ "$2" = -json ]; then
      if [ "${TERRACOTTA_FAKE_MODE:-success}" = no_changes ]; then
        printf '%s\n' '{"format_version":"1.0","applyable":false}'
      else
        cat "$TERRACOTTA_FAKE_SHOW_JSON"
      fi
    else
      if [ "${TERRACOTTA_FAKE_MODE:-success}" = no_changes ]; then
        printf '%s\n' 'No changes. Your infrastructure matches the configuration.'
      else
        cat "$TERRACOTTA_FAKE_SHOW_TEXT"
      fi
    fi
    ;;
  *)
    exit 2
    ;;
esac
