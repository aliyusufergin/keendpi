#!/bin/sh
# NDM netfilter hook: NDM has just rewritten table $table for $type; put keendpi's rules back.
[ "$type" = iptables ] && [ "$table" = mangle ] || exit 0
PIDF=/tmp/keendpi.pid
[ -f $PIDF ] && tr '\0' ' ' < /proc/$(cat $PIDF)/cmdline 2>/dev/null | grep -q "^/opt/sbin/keendpi run " || exit 0
out=$(/opt/share/keendpi/rules.sh add 2>&1)
rc=$?
# 3 = no internet connection yet; rules.sh retries by itself.
[ $rc = 0 ] || [ $rc = 3 ] || logger -t keendpi "netfilter hook: rules not restored ($rc): $out"
exit 0
