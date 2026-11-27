#!/bin/sh
# NDM netfilter hook: NDM has just rewritten table $table for $type; put gecit's rules back.
[ "$type" = iptables ] && [ "$table" = mangle ] || exit 0
PIDF=/tmp/gecit.pid
[ -f $PIDF ] && tr '\0' ' ' < /proc/$(cat $PIDF)/cmdline 2>/dev/null | grep -q "^/opt/sbin/gecit run " || exit 0
out=$(/opt/share/gecit/rules.sh add 2>&1)
rc=$?
# 3 = no internet connection yet; rules.sh retries by itself.
[ $rc = 0 ] || [ $rc = 3 ] || logger -t gecit "netfilter hook: rules not restored ($rc): $out"
exit 0
