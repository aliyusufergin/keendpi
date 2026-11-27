#!/bin/sh
# Publish HEAD's tree, minus the private work record, as a commit on branch `public`, then push it
# to origin's main: tools/sync-public.sh "message"
# The local history (with .work/ and CLAUDE.md: router layout, notes) never leaves this machine.
set -e
cd "$(dirname "$0")/.."
MSG=${1:?usage: $0 "commit message"}
[ -z "$(git status --porcelain)" ] || { echo "commit your changes first"; exit 1; }
tree=$(git ls-tree HEAD | grep -v -P '\t(\.work|CLAUDE\.md)$' | git mktree)
parent=$(git rev-parse -q --verify refs/heads/public || true)
if [ -n "$parent" ] && [ "$(git rev-parse "$parent^{tree}")" = "$tree" ]; then
  echo "public is up to date"
else
  c=$(git commit-tree "$tree" ${parent:+-p "$parent"} -m "$MSG")
  git update-ref refs/heads/public "$c"
fi
git push origin public:main
