#!/usr/bin/env bash
# v0.1.0 demo superseded: the Trigger-core demo is now the canonical one.
# This wrapper keeps the old entry point working.
exec "$(dirname "$0")/trigger-demo.sh" "$@"
