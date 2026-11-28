# keendpi

[![build](https://github.com/aliyusufergin/keendpi/actions/workflows/build.yml/badge.svg)](https://github.com/aliyusufergin/keendpi/actions/workflows/build.yml)
[![release](https://img.shields.io/github/v/release/aliyusufergin/keendpi)](https://github.com/aliyusufergin/keendpi/releases)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

Transparent DPI circumvention for Keenetic routers.

keendpi runs on the router under Entware and gets connections past SNI- and Host-based blocking for every device on the network. Devices need no client, proxy or VPN, and traffic does not leave the normal route.

[Türkçe](README.tr.md)

## Features

- **Network-wide.** Every LAN device benefits, with no per-device setup.
- **Low overhead.** Only the first packets of each connection are inspected, and the rest stays on Keenetic's fast path. On the reference device, median ping rose by at most 0.1 ms and throughput stayed above 98 %.
- **Fail-open.** If the engine stops or hangs, traffic passes untouched. A watchdog restarts a hung engine within about a second.
- **Self-learning.** In `auto` mode, blocked domains are detected from forged resets and injected block pages, and added to a list.
- **Persistent.** Rules survive reboots, WAN reconnects and the firmware rewriting its netfilter tables.
- **Clean uninstall.** Removing the package removes everything it added.

## How it works

DPI middleboxes read the server name from the first data packet of a connection: the TLS ClientHello (SNI) or the HTTP `Host` header. When the name is on a blocklist, they reset the connection or inject a redirect.

keendpi queues the first request of each connection via NFQUEUE. For connections that should be protected, it sends a decoy request ahead of the real one:

- The decoy is a ClientHello or HTTP request for a harmless name (`www.google.com` by default).
- Its TTL is low enough to pass the DPI box but expire before the server. The TTL is capped from the hop distance measured on the SYN-ACK.

The DPI box classifies the connection on the decoy, and the real request then passes unchanged. keendpi never modifies or drops the original packets. Only the first request and the handshake packets of a connection pass through the engine, which adds well under a millisecond to connection setup.

TLS is handled on any TCP port, and plain HTTP on port 80. keendpi makes no outbound connections of its own and collects no telemetry.

## Compatibility

### Routers

| Entware architecture (`opkg print-architecture`) | Status |
|---|---|
| `aarch64-3.10` | Tested: Keenetic Hopper KN-3811, KeeneticOS 5.1 |
| `mipsel-3.4` | Builds; untested |
| `mips-3.4` (big-endian) | Not supported |

### ISPs (Turkey)

| ISP | Status | Notes |
|---|---|---|
| Türksat Kablonet | Tested | TCP resets and "Güvenli İnternet" redirects. `TTL=3` (2 also works). |
| Türk Telekom | Untested | Reported to reset like Kablonet. Try the defaults, then `TTL` 2–5. |
| Vodafone | Untested | Reported to reset. Try the defaults, then `TTL` 2–5. |
| Superonline | Untested | Reported to use a different DPI vendor. |
| TurkNet | Untested | Reported to block via DNS only; encrypted DNS may suffice. |

Reports for other ISPs and models are welcome; see [Contributing](#contributing).

## Requirements

1. **Firmware components.** These must be installed: *OPKG package manager*, *Netfilter subsystem kernel modules*, and *DNS-over-TLS proxy* or *DNS-over-HTTPS proxy*. Entware must be installed on USB storage (see Keenetic's Entware guide on help.keenetic.com), with SSH access (default `ssh root@192.168.1.1 -p 222`).

2. **Encrypted DNS.** ISPs also tamper with DNS, and some intercept plain DNS to any resolver. keendpi does not handle DNS, so configure DoT or DoH on the router and remove the plain DNS servers. From Entware:

   ```sh
   ndmc -c "dns-proxy tls upstream 1.1.1.1 853 sni cloudflare-dns.com"
   ndmc -c "no ip name-server 1.1.1.1"           # repeat for each plain DNS server
   ndmc -c "copy running-config startup-config"
   ```

   Also turn on *Ignore DNS from provider* for the WAN connection, and make sure DHCP does not hand out a separate DNS server. `nslookup discord.com` on the router should return a real address, not a block-page address.

3. **No other DPI tool** (zapret, nfqws, ByeDPI, …) running on the router.

## Installation

Run these in an interactive SSH session on Entware:

```sh
opkg update
opkg list-installed | grep -q '^wget-ssl ' || opkg install wget-ssl ca-certificates
echo "src/gz keendpi https://aliyusufergin.github.io/keendpi/opkg" > /opt/etc/opkg/keendpi.conf
opkg update
opkg install keendpi
```

The line `Package keendpi … has no valid architecture, ignoring` is expected: the feed carries several architectures.

The service starts immediately and at every boot. Verify it with:

```sh
/opt/etc/init.d/S51keendpi check
```

Packages are also attached to each [release](https://github.com/aliyusufergin/keendpi/releases).

## Configuration

The configuration lives in `/opt/etc/keendpi/keendpi.conf`. Apply changes with `/opt/etc/init.d/S51keendpi restart`. List files are reloaded automatically within a few seconds.

| Key | Default | Description |
|---|---|---|
| `ENABLED` | `yes` | `no` keeps the package installed but inactive. |
| `MODE` | `auto` | `list`, `auto` or `all` (see below). |
| `LISTS` | `/opt/etc/keendpi/lists` | Domain list file, or a directory of `*.txt` files. |
| `EXCLUDE` | `/opt/etc/keendpi/exclude.txt` | Domains never touched, in any mode. |
| `AUTO_LIST` | `/opt/etc/keendpi/auto.txt` | Domains learned in `auto` mode. |
| `HTTP` | `yes` | Also handle plain HTTP on port 80. |
| `TTL` | `3` | Decoy TTL when leaving the router. It is lowered automatically for nearer servers. |
| `FAKE_HOST` | `www.google.com` | Server name used in decoys. |
| `WAN` | `auto` | WAN interface(s). `auto` uses the default route's interface. |
| `VERBOSE` | `no` | Log every handled connection. |
| `QUEUE`, `MARK`, `FORCE_MARK` | `200`, `0x40000000`, `0x20000000` | NFQUEUE number and packet-mark bits. |

**List format.** One domain per line. Subdomains are included, and `#` starts a comment.

| File | Purpose |
|---|---|
| `lists/default.txt` | Shipped list; replaced on upgrade. |
| `lists/*.txt` | Your own lists, for example `lists/user.txt`; kept on upgrade. |
| `exclude.txt` | Domains to leave alone. |
| `auto.txt` | Learned domains; delete a line to forget one. |

### Modes

| Mode | Connections handled |
|---|---|
| `list` | Domains in `LISTS`. |
| `auto` | `LISTS` plus learned domains. The first blocked connection to a new domain teaches it, and the next one succeeds. |
| `all` | Every TLS and HTTP connection. |

`EXCLUDE` takes precedence in every mode.

In `auto` mode, a connection counts as blocked in two cases:
- a reset arrives in less than half the measured round-trip time, so the server could not have sent it;
- the reply to a ClientHello is plain HTTP, which is an injected block page.

## Usage

```sh
/opt/etc/init.d/S51keendpi {start|stop|restart|status}
/opt/etc/init.d/S51keendpi check [domain ...]   # compare direct vs. keendpi
/opt/sbin/keendpi validate                      # check the configuration
```

`check` makes two HTTPS connections (port 443) per domain from the router: one direct and one through keendpi. It then tests a control domain. Blocks on other ports or on plain HTTP are not tested.

| Result | Meaning |
|---|---|
| `ok` | The server answered. |
| `blocked (reset)` | Reset by a middlebox. |
| `blocked (block page)` | Redirected to a block page. |
| `blocked (closed)`, `blocked (no answer)` | Closed, or no reply. |
| `no connection` | TCP connect failed: DNS returned a block address, the IP is blocked, or the host is down. |
| `no DNS answer` | The name does not resolve. |
| `engine not running` | The service is stopped. |

`direct: blocked` together with `with keendpi: ok` means keendpi works for that domain.

## Upgrading and uninstalling

```sh
opkg update && opkg upgrade keendpi    # configuration and lists are kept
opkg remove --autoremove keendpi       # removes rules, files, the feed entry and iptables
```

Uninstalling deletes `/opt/etc/keendpi`, including your lists. Avoid `opkg install --force-reinstall`: opkg treats it as remove-then-install, which deletes the configuration.

## Troubleshooting

- **Logs:** run `ndmc -c "show log" | grep keendpi`, or open the system log in the web interface. Set `VERBOSE=yes` for per-connection detail.
- **Rules:** run `iptables -t mangle -S | grep keendpi`.
- **A site still fails:** run `check` for it.
  - `with keendpi: blocked`: try `TTL` values 2, 4 or 5.
  - `no connection`: the block is not SNI-based (DNS or IP).
  - `auto` mode: retry once, since the first attempt is used for learning.
- **A site breaks:** add it to `exclude.txt`.

## Limitations

- IP-based blocking and throttling cannot be bypassed; keendpi is not a VPN.
- DNS-based blocking requires encrypted DNS on the router.
- Only IPv4 TCP is handled. QUIC is not.
- If a ClientHello's SNI falls outside its first segment, a decoy is sent regardless of lists, so `EXCLUDE` cannot apply to that connection. This is harmless.
- In `auto` mode, blocks on servers less than about 3 ms away cannot be told apart from server resets. List such domains manually.

## Building from source

The build uses Docker, so no local Rust toolchain is needed.

```sh
docker/build.sh test
docker/build.sh build --release --target aarch64-unknown-linux-musl
tools/mkipk.sh            # → dist/keendpi_<version>-1_aarch64-3.10.ipk
```

| Path | Contents |
|---|---|
| `src/engine.rs` | Packet handling, decoys, learning |
| `src/supervise.rs` | Worker supervision and watchdog |
| `src/check.rs` | `check` command |
| `files/rules.sh` | Netfilter rules |
| `files/S51keendpi` | Init script |
| `files/50-keendpi.sh` | NDM netfilter hook |

Release steps are in [RELEASING.md](RELEASING.md), and changes are in [CHANGELOG.md](CHANGELOG.md).

## Contributing

Issues and pull requests are welcome. When reporting a site, ISP or router model, use the [issue template](https://github.com/aliyusufergin/keendpi/issues/new/choose) and include the `check` output.

## Disclaimer

keendpi is a networking tool. You are responsible for complying with the laws that apply to you. The software is provided "as is", without warranty of any kind.

## License

Licensed under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option.
