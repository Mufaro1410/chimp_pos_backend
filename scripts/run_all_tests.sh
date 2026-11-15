#!/usr/bin/env bash
# Runs every end-to-end suite against a running server. Use a DEVELOPMENT/STAGING server, never production:
# the suites create throw-away companies.
#
# Start the server like this (the settings the suites rely on):
#   REFRESH_ROTATION_GRACE_SECONDS=2 PASSWORD_RESET_DELIVERY=log AUTH_RATE_LIMIT_PER_MINUTE=100000 \
#   RUN_MIGRATIONS=true cargo run 2>&1 | tee /tmp/server.log
#
# then:   ./scripts/run_all_tests.sh [BASE_URL] [SERVER_LOG]
set -u
BASE="${1:-http://127.0.0.1:3000}"
LOG="${2:-/tmp/server.log}"
cd "$(dirname "$0")/.."
rc=0
for t in smoke_test offline_sync_test auth_test concurrency_test; do
  echo "================ $t"
  if [ "$t" = auth_test ]; then python3 scripts/$t.py "$BASE" "$LOG" || rc=1; else python3 scripts/$t.py "$BASE" || rc=1; fi
done
echo
[ $rc -eq 0 ] && echo "ALL SUITES PASSED" || echo "SOME SUITES FAILED"
exit $rc
