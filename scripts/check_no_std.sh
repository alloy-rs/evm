#!/usr/bin/env bash
set -eo pipefail

no_std_packages=(
  alloy-evm
  alloy-op-evm
)

for package in "${no_std_packages[@]}"; do
  if [ -n "$CI" ]; then
    echo "::group::cargo +stable build -p $package --target riscv32imac-unknown-none-elf --no-default-features"
  else
    printf "\n%s:\n  %s\n" "$package" "cargo +stable build -p $package --target riscv32imac-unknown-none-elf --no-default-features"
  fi

  cargo +stable build -p "$package" --target riscv32imac-unknown-none-elf --no-default-features

  [ -n "$CI" ] && echo "::endgroup::"
done
