#!/usr/bin/env bash
# tools/smoke_all.sh — Run all AUREA MVP smoke tests
# Usage: bash tools/smoke_all.sh [BASE_URL]
set -euo pipefail

BASE_URL="${1:-http://localhost:8080}"
ARTIFACTS="artifacts/smoke"
mkdir -p "$ARTIFACTS"

PASS=0; FAIL=0

run() {
    local name="$1"
    local script="$2"
    echo "━━━ [$name] ━━━"
    if bash "$script" "$BASE_URL"; then
        echo "✓ $name PASSED"
        PASS=$((PASS+1))
    else
        echo "✗ $name FAILED"
        FAIL=$((FAIL+1))
    fi
    echo ""
}

run "A-Idempotência"    tools/smoke_idem.sh
run "B-Auto-Repair-N2" tools/smoke_repair.sh
run "C-DUAL_CONTROL"   tools/smoke_dual_control.sh
run "D-Policy-Blocked" tools/smoke_policy.sh
run "E-Verify-Anchor"  tools/smoke_verify.sh
run "F-Métricas-SLO"   tools/smoke_metrics.sh
run "G-Export"         tools/smoke_export.sh

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Results: $PASS passed, $FAIL failed"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ "$FAIL" -gt 0 ]; then exit 1; fi
