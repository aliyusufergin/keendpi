#!/bin/sh
# Build the opkg package: tools/mkipk.sh [ARCH] [BINARY]   -> dist/gecit_<version>_<arch>.ipk
# ARCH is the Entware architecture (aarch64-3.10, mipsel-3.4, mips-3.4).
set -e
cd "$(dirname "$0")/.."
ARCH=${1:-aarch64-3.10}
BIN=${2:-target/aarch64-unknown-linux-musl/release/gecit}
VER=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)-1
[ -x "$BIN" ] || { echo "no binary $BIN; build it first"; exit 1; }

W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
D=$W/data
install -D -m 755 "$BIN" $D/opt/sbin/gecit
install -D -m 644 files/gecit.conf $D/opt/etc/gecit/gecit.conf
install -D -m 644 files/exclude.txt $D/opt/etc/gecit/exclude.txt
install -D -m 644 lists/default.txt $D/opt/etc/gecit/lists/default.txt
install -D -m 755 files/S51gecit $D/opt/etc/init.d/S51gecit
install -D -m 755 files/50-gecit.sh $D/opt/etc/ndm/netfilter.d/50-gecit.sh
install -D -m 755 files/rules.sh $D/opt/share/gecit/rules.sh

C=$W/control; mkdir -p $C
cat > $C/control <<EOF
Package: gecit
Version: $VER
Depends: iptables
Section: net
Architecture: $ARCH
Maintainer: aliyusufergin
License: MIT OR Apache-2.0
Installed-Size: $(du -sk $D | cut -f1)
Description: DPI-circumvention engine for Keenetic routers (Discord and other SNI/HTTP blocks)
EOF
# User-editable files: kept on upgrade. lists/default.txt is ours and is replaced on upgrade.
printf '/opt/etc/gecit/gecit.conf\n/opt/etc/gecit/exclude.txt\n' > $C/conffiles
cat > $C/postinst <<'EOF'
#!/bin/sh
# Starts unless the config says ENABLED=no (which also keeps a disabled service off on upgrade).
/opt/etc/init.d/S51gecit restart
exit 0
EOF
cat > $C/prerm <<'EOF'
#!/bin/sh
/opt/etc/init.d/S51gecit stop
exit 0
EOF
cat > $C/postrm <<'EOF'
#!/bin/sh
# On removal (not upgrade) leave nothing behind: config, lists, learned domains, pid file.
[ "$1" = upgrade ] && exit 0
# The feed file is the one the README tells users to create; without the package it is useless.
rm -rf /opt/etc/gecit /opt/share/gecit /tmp/gecit.pid /tmp/gecit.rules.lock /tmp/gecit.rules.retry \
  /opt/etc/opkg/gecit.conf /opt/var/opkg-lists/gecit
exit 0
EOF
chmod 755 $C/postinst $C/prerm $C/postrm

echo "2.0" > $W/debian-binary
T="tar --owner=0 --group=0 --numeric-owner --sort=name --mtime=@0"
# Directory entries only for our own directories (opkg creates no missing parents); shared ones
# such as an empty /opt/etc/ndm/netfilter.d must not be listed, or removal would rmdir them.
OWN="./opt/etc/gecit ./opt/etc/gecit/lists ./opt/share/gecit"
(cd $D && { for d in $OWN; do echo $d; done; find . -type f; } | sort | $T -czf $W/data.tar.gz --no-recursion -T -)
(cd $C && $T -czf $W/control.tar.gz .)
mkdir -p dist
OUT=dist/gecit_${VER}_${ARCH}.ipk
(cd $W && $T -czf - ./debian-binary ./control.tar.gz ./data.tar.gz) > $OUT
echo "$OUT ($(du -k $OUT | cut -f1) KB)"
