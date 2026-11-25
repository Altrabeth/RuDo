#!/bin/sh
# Smoke test of a RuDo Docker image without a terminal, as CI and the release run it.
#
# Runs `--version` and `config check`, and checks that the interface, started without `-it`,
# fails at once with a hint instead of waiting for input.
#
# Usage: scripts/docker-smoke.sh IMAGE
set -eu

image="${1:?usage: scripts/docker-smoke.sh IMAGE}"

docker run --rm "$image" --version
docker run --rm "$image" config check

# A hang ends after the timeout with status 124 and no hint, which fails below as well.
if output="$(timeout 60 docker run --rm "$image" 2>&1)"; then
    echo "error: the interface started without a terminal" >&2
    exit 1
fi
case "$output" in
    *"needs an interactive terminal"*) ;;
    *)
        echo "error: unexpected output without a terminal: $output" >&2
        exit 1
        ;;
esac
echo "docker smoke test passed: --version, config check and the hint without a terminal"
