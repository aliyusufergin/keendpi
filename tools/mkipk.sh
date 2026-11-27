#!/bin/sh
# Build the opkg package: tools/mkipk.sh [ARCH] [BINARY]   -> dist/keendpi_<version>_<arch>.ipk
# ARCH is the Entware architecture (aarch64-3.10, mipsel-3.4, mips-3.4).
set -e
cd "$(dirname "$0")/.."
ARCH=${1:-aarch64-3.10}
BIN=${2:-target/aarch64-unknown-linux-musl/release/keendpi}
VER=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)-1
[ -x "$BIN" ] || { echo "no binary $BIN; build it first"; exit 1; }

W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
D=$W/data
install -D -m 755 "$BIN" $D/opt/sbin/keendpi
install -D -m 644 files/keendpi.conf $D/opt/etc/keendpi/keendpi.conf
install -D -m 644 files/exclude.txt $D/opt/etc/keendpi/exclude.txt
install -D -m 644 lists/default.txt $D/opt/etc/keendpi/lists/default.txt
install -D -m 755 files/S51keendpi $D/opt/etc/init.d/S51keendpi
install -D -m 755 files/50-keendpi.sh $D/opt/etc/ndm/netfilter.d/50-keendpi.sh
install -D -m 755 files/rules.sh $D/opt/share/keendpi/rules.sh

C=$W/control; mkdir -p $C
cat > $C/control <<EOF
Package: keendpi
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
printf '/opt/etc/keendpi/keendpi.conf\n/opt/etc/keendpi/exclude.txt\n' > $C/conffiles
cat > $C/postinst <<'EOF'
#!/bin/sh
# Starts unless the config says ENABLED=no (which also keeps a disabled service off on upgrade).
/opt/etc/init.d/S51keendpi restart
exit 0
EOF
cat > $C/prerm <<'EOF'
#!/bin/sh
/opt/etc/init.d/S51keendpi stop
exit 0
EOF
cat > $C/postrm <<'EOF'
#!/bin/sh
# On removal (not upgrade) leave nothing behind: config, lists, learned domains, pid file.
[ "$1" = upgrade ] && exit 0
# The feed file is the one the README tells users to create; without the package it is useless.
rm -rf /opt/etc/keendpi /opt/share/keendpi /tmp/keendpi.pid /tmp/keendpi.rules.lock /tmp/keendpi.rules.retry \
  /opt/etc/opkg/keendpi.conf /opt/var/opkg-lists/keendpi
exit 0
EOF
chmod 755 $C/postinst $C/prerm $C/postrm

echo "2.0" > $W/debian-binary
T="tar --owner=0 --group=0 --numeric-owner --sort=name --mtime=@0"
# Directory entries only for our own directories (opkg creates no missing parents); shared ones
# such as an empty /opt/etc/ndm/netfilter.d must not be listed, or removal would rmdir them.
OWN="./opt/etc/keendpi ./opt/etc/keendpi/lists ./opt/share/keendpi"
(cd $D && { for d in $OWN; do echo $d; done; find . -type f; } | sort | $T -czf $W/data.tar.gz --no-recursion -T -)
(cd $C && $T -czf $W/control.tar.gz .)
mkdir -p dist
OUT=dist/keendpi_${VER}_${ARCH}.ipk
(cd $W && $T -czf - ./debian-binary ./control.tar.gz ./data.tar.gz) > $OUT
echo "$OUT ($(du -k $OUT | cut -f1) KB)"
