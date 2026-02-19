#!/usr/bin/env bash
set -euo pipefail
pip install mkdocs mkdocs-material >/dev/null
mkdocs build --strict
mkdocs gh-deploy --force
