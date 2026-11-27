#!/bin/sh
# keendpi netfilter rules: rules.sh add|del   (config: $KEENDPI_CONF, default /opt/etc/keendpi/keendpi.conf)
# Used by the init script and by the NDM netfilter.d hook, which re-runs `add` whenever the
# router rewrites its tables. `add` is idempotent; `del` removes everything `add` created.
#
# keendpi_out (mangle POSTROUTING -o WAN): the first request of a connection only: a TLS ClientHello
#   on any TCP port (filters also watch other TLS ports, e.g. GeForce NOW's 48322) or an HTTP request
#   to port 80. keendpi_q marks the connection (connmark bit MARK; NDM keeps its own marks in
#   ndmmark) so later requests skip. In auto mode also SYNs, to time the round trip that tells the
#   DPI's resets apart. One or two packets per connection; the rest never reach the queue.
# keendpi_in (mangle FORWARD and INPUT -i WAN): conntrack-ESTABLISHED SYN-ACKs (hop distance,
#   round trip) and, in auto mode, RSTs and plain-HTTP answers from ports other than 80 (the
#   block signals). Nothing unsolicited.
# The jumps are appended at the end of each chain: an NFQUEUE verdict ends the chain, so NDM's
# own mangle rules (MSS clamping, marks) must run before ours.
# --queue-bypass: with no engine listening, packets pass untouched.
PATH=/opt/sbin:/opt/bin:/opt/usr/sbin:/opt/usr/bin:/usr/sbin:/usr/bin:/sbin:/bin
CONF=${KEENDPI_CONF:-/opt/etc/keendpi/keendpi.conf}
BIN=/opt/sbin/keendpi
PIDF=/tmp/keendpi.pid
MODMARK=/tmp/keendpi.insmod   # modules we inserted; /tmp is RAM, cleared at reboot like the modules
LOCK=/tmp/keendpi.rules.lock
RETRY=/tmp/keendpi.rules.retry
IPT="iptables -w -t mangle"   # -w: wait for the xtables lock NDM may hold
U32_TLS='0>>22&0x3C@12>>26&0x3C@0>>24=0x16 && 0>>22&0x3C@12>>26&0x3C@5>>24=0x01'   # ClientHello
U32_HTTP="0x47455420 0x504F5354 0x48454144"   # "GET ", "POST", "HEAD"
U32_HTTP_REPLY='0>>22&0x3C@12>>26&0x3C@0=0x48545450'   # "HTTP"

say() { echo "keendpi: $*" >&2; logger -t keendpi "$*" 2>/dev/null; }

# The engine parses the config; the shell only evals its normalised output.
vars() {
  v=$($BIN shellvars -c "$CONF" 2>&1) || { say "config: $v"; return 1; }
  eval "$v"
  NFQ="-j NFQUEUE --queue-num $QUEUE --queue-bypass"
}

# One rules.sh at a time (init script, NDM hook and retry loop can overlap).
lock() {
  i=0
  until mkdir $LOCK 2>/dev/null; do
    i=$((i + 1))
    [ $i -ge 30 ] && { rm -rf $LOCK; i=0; continue; }   # holder hung or was killed: take over
    sleep 1
  done
}
unlock() { rm -rf $LOCK; }

modpath() {
  for p in /lib/modules/$(uname -r) /lib/system-modules/$(uname -r) /lib/system-modules; do
    [ -f "$p/$1.ko" ] && { echo "$p/$1.ko"; return; }
  done
}

wans() {
  if [ "$WAN" = auto ]; then
    # Busybox ip ignores "show default" and lists every route, so filter here.
    ip route 2>/dev/null | awk '$1 == "default" {for (i = 1; i < NF; i++) if ($i == "dev") print $(i + 1)}' | sort -u
  else
    echo $WAN
  fi
}

# Remove every jump to our chains, whatever interface it names (the WAN may have changed).
unhook() {
  for h in POSTROUTING FORWARD INPUT; do
    $IPT -S $h 2>/dev/null | grep -E -- '-j keendpi_(out|in)$' | sed 's/^-A //' | while read -r r; do
      $IPT -D $r 2>/dev/null
    done
  done
}

# No WAN yet (boot, reconnect): try again in the background until it is up or keendpi stops.
retry_later() {
  mkdir $RETRY 2>/dev/null || return 0   # one retry loop is enough
  (
    for i in $(seq 1 90); do
      sleep 10
      [ -f $PIDF ] || break
      [ -n "$(wans)" ] && { "$0" add; break; }
    done
    rm -rf $RETRY
  ) </dev/null >/dev/null 2>&1 &
}

add_locked() {
  w=$(wans)
  if [ -z "$w" ]; then
    retry_later
    return 3
  fi
  # The queue modules are usually loaded already; xt_u32 usually is not.
  for m in nfnetlink_queue xt_NFQUEUE xt_u32; do
    grep -q "^$m " /proc/modules && continue
    p=$(modpath $m)
    [ -n "$p" ] || { say "kernel module $m not found (install the Netfilter kernel modules component)"; return 1; }
    if insmod "$p" 2>/dev/null; then
      echo $m >> $MODMARK
    else
      grep -q "^$m " /proc/modules || { say "cannot load $m"; return 1; }
    fi
  done
  for c in keendpi_out keendpi_q keendpi_in; do $IPT -N $c 2>/dev/null; $IPT -F $c || return 1; done
  $IPT -A keendpi_q -j CONNMARK --set-xmark $MARK/$MARK
  $IPT -A keendpi_q $NFQ
  $IPT -A keendpi_out -m mark --mark $MARK/$MARK -j RETURN
  $IPT -A keendpi_out -m connmark --mark $MARK/$MARK -j RETURN
  [ "$LEARN" = yes ] && $IPT -A keendpi_out -p tcp --tcp-flags SYN,ACK,RST SYN $NFQ
  $IPT -A keendpi_out -p tcp -m u32 --u32 "$U32_TLS" -j keendpi_q
  if [ "$HTTP" = yes ]; then
    for x in $U32_HTTP; do
      $IPT -A keendpi_out -p tcp --dport 80 -m u32 --u32 "0>>22&0x3C@12>>26&0x3C@0=$x" -j keendpi_q
    done
  fi
  $IPT -A keendpi_in -p tcp --tcp-flags SYN,ACK,RST SYN,ACK -m conntrack --ctstate ESTABLISHED $NFQ
  [ "$LEARN" = yes ] && $IPT -A keendpi_in -p tcp --tcp-flags RST RST -m conntrack --ctstate ESTABLISHED $NFQ
  # A filter's block page instead of a ServerHello: TCP payload starting "HTTP" from a TLS port.
  [ "$LEARN" = yes ] && $IPT -A keendpi_in -p tcp ! --sport 80 -m u32 --u32 "$U32_HTTP_REPLY" -m conntrack --ctstate ESTABLISHED $NFQ
  unhook
  for i in $w; do
    $IPT -A POSTROUTING -o $i -j keendpi_out || return 1
    for h in FORWARD INPUT; do $IPT -A $h -i $i -j keendpi_in || return 1; done
  done
}

del_locked() {
  unhook
  for c in keendpi_out keendpi_q keendpi_in; do $IPT -F $c 2>/dev/null; done
  for c in keendpi_out keendpi_q keendpi_in; do $IPT -X $c 2>/dev/null; done
  if [ -f $MODMARK ]; then
    for m in $(cat $MODMARK); do grep -q "^$m " /proc/modules && rmmod $m 2>/dev/null; done
    rm -f $MODMARK
  fi
  ! $IPT -S 2>/dev/null | grep -q keendpi
}

case "$1" in
  add)
    vars || exit 1
    lock
    # NDM may rewrite the table while we fill it (seen at boot: "Couldn't load target keendpi_out").
    for try in 1 2 3 4 5; do
      add_locked; rc=$?
      [ $rc = 1 ] || break
      sleep 2
    done
    unlock
    exit $rc ;;
  del) lock; del_locked; rc=$?; unlock; exit $rc ;;
  *) echo "usage: $0 add|del" >&2; exit 2 ;;
esac
