#!/bin/sh
# Print a snapshot of router state relevant to install/uninstall residue checks.
# Run on the router: ssh root@ROUTER -p 222 sh -s < tools/snapshot.sh > snap.txt
sec() { echo "=== $1"; }
sec files;   find /opt -xdev \( -path /opt/var/run -o -path /opt/tmp -o -path /opt/var/log -o -path /opt/var/opkg-lists \) -prune -o -type f -print 2>/dev/null | sort | while read f; do md5sum "$f"; done
echo "--- /opt/tmp entries"; ls -a /opt/tmp 2>/dev/null
sec dirs;    find /opt -xdev -type d 2>/dev/null | sort
sec links;   find /opt -xdev -type l 2>/dev/null | sort
sec opkg;    opkg list-installed 2>/dev/null
sec modules; cut -d' ' -f1 /proc/modules | sort
sec targets; sort -u /proc/net/ip_tables_targets
sec matches; sort -u /proc/net/ip_tables_matches
sec sysctl;  sysctl -a 2>/dev/null | grep -v fastopen_key | grep -E '^net\.(netfilter|ipv4\.(ip_forward|tcp_|conf\.all)|core\.(rmem|wmem))' | sort
sec iptables
# Without Entware iptables the rules can't be read: install it just for this, then remove exactly what was added.
tmp_added=""
if ! command -v iptables-save >/dev/null; then
  before=$(opkg list-installed | cut -d' ' -f1 | sort)
  opkg install iptables >/dev/null 2>&1 || echo "ERROR: cannot install iptables to read rules"
  tmp_added=$(opkg list-installed | cut -d' ' -f1 | sort | awk -v b="$before" 'BEGIN{n=split(b,a,"\n");for(i=1;i<=n;i++)old[a[i]]=1} !old[$0]')
fi
for c in iptables-save ip6tables-save; do echo "--- $c"; $c 2>&1 | grep -v '^#' | sed -E 's/\[[0-9]+:[0-9]+\]/[0:0]/'; done
for pass in 1 2 3; do next=""; for p in $tmp_added; do opkg remove "$p" >/dev/null 2>&1 || next="$next $p"; done; tmp_added=$next; done
[ -n "$(echo $tmp_added)" ] && echo "ERROR: could not remove temporary packages: $tmp_added"
sec ndm-config; ndmc -c 'show running-config' 2>/dev/null | grep -v -E '^\s*!|^\[K' \
  | sed -E 's/((password|secret|psk|key|passphrase|token|hash)[^ ]* ).*/\1<masked>/I'
sec processes; ps w 2>/dev/null | awk 'NR>1{ $1=""; print }' | grep -v -E 'ps w|awk|sort|sed -E|snapshot|sh -s' | sed -E 's/^ +[^ ]+ +[^ ]+ +[^ ]+ +//' | sort -u
