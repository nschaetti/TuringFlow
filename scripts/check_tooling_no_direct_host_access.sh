#!/usr/bin/env bash
set -euo pipefail

PATTERN='std::fs|std::process::Command|tokio::fs|tokio::process|reqwest::blocking::Client|image::open\('

if grep -RInE "$PATTERN" src/commands src/rchain/tools.rs --exclude='runtime.rs'; then
  echo "Direct host access detected in tools runtime perimeter."
  echo "Use kernel wrappers/providers instead."
  exit 1
fi

echo "Tooling runtime host-access lint passed."
