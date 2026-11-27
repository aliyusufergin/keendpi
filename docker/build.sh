#!/bin/sh
# Build keendpi inside a container; artefacts land in ./target. Usage: docker/build.sh [cargo args]
set -e
cd "$(dirname "$0")/.."
docker build -q -t keendpi-build -f docker/build.Dockerfile docker >/dev/null
exec docker run --rm -u "$(id -u):$(id -g)" -e CARGO_HOME=/src/target/.cargo-home \
  -v "$PWD":/src keendpi-build cargo "$@"
