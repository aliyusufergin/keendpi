# gecit

**DPI circumvention for Keenetic routers.** Discord, sites blocked by SNI or HTTP Host, and Turkey's "Güvenli İnternet" filter open on **every device on the network** (phones, TVs, consoles, guests), with nothing to set up on the devices. It is not a VPN: your traffic does not go through anyone else's server.

[Türkçe](README.md)

- A small engine running on the router (Rust, ~600 KB). It looks at only the first few packets of a connection; the rest takes Keenetic's fast path (FASTNAT). In measurements it added at most +0.1 ms of ping and kept over 98 % of throughput.
- If the engine stops or crashes, the internet keeps working; only the circumvention is lost.
- It carries on by itself after a router reboot and after the internet connection drops and comes back.
- Uninstalling leaves no trace.

> Status: tested on Türksat Kablonet (PPPoE) with a Keenetic Hopper KN-3811. Other ISPs and models are untested; see [ISPs](#isps) and [Models](#models).

## Contents

- [How it works](#how-it-works)
- [Requirements](#requirements)
- [Install](#install)
- [Configuration](#configuration)
- [List modes](#list-modes)
- [Check command](#check-command)
- [Upgrade](#upgrade)
- [Uninstall](#uninstall)
- [Troubleshooting](#troubleshooting)
- [ISPs](#isps)
- [Models](#models)
- [Limits](#limits)
- [Development](#development)
- [Licence](#licence)

## How it works

The ISP's DPI box reads the site name (SNI) in the first packet of an HTTPS connection (the TLS ClientHello). When it sees a blocked site, it resets the connection or redirects it to a block page.

gecit sends a **fake first packet** just before the real first packet of a connection to a blocked site. The fake carries a harmless name (`www.google.com`) and a TTL low enough that it dies on the way after passing the DPI box. The DPI box sees the fake and lets the connection go; the fake never reaches the server. The real packet goes out unchanged. It does the same for plain HTTP (port 80).

- Lists decide which connections get this (in `auto` mode gecit also learns blocked sites by itself).
- It measures how many hops away the server is and picks a TTL that can never reach the server.
- Nothing leaves the router: gecit connects to no one, and there is no telemetry.

## Requirements

1. **OPKG and Entware.** In the Keenetic web interface, under the component options of the general system settings, these components must be installed:
   - **OPKG package manager**,
   - **Netfilter subsystem kernel modules**.

   Entware must be installed on a USB drive (search for "Entware" on help.keenetic.com for the official guide). You need SSH access to Entware (usually `ssh root@192.168.1.1 -p 222`).

2. **Encrypted DNS (DoH/DoT).** ISPs also answer DNS queries for blocked sites with a wrong address. Kablonet intercepts plain DNS on the way (including queries to 1.1.1.1 and 8.8.8.8) and returns its block page's address. gecit does not touch DNS. On the Keenetic (menu names vary by version):
   - in the internet safety / DNS configuration section, add a **DNS-over-TLS** or **DNS-over-HTTPS** server (e.g. `1.1.1.1:853`, SNI `cloudflare-dns.com`);
   - remove plain DNS servers, and turn on **ignore the provider's DNS** in the internet connection settings;
   - if DHCP hands a separate DNS server to devices (e.g. `1.1.1.1`), remove it so devices use the router.

   To verify: on Entware, `nslookup discord.com` must give a real address (`162.159.x.x`), not a block address such as `195.175.254.2`.

3. No other DPI tool (zapret, nfqws, ByeDPI, …) may **run** on the router; they work on the same packets.

## Install

Connect to Entware over SSH and run:

```sh
# 1. A wget that can do HTTPS (Entware's own wget cannot)
opkg update
opkg install wget-ssl

# 2. Add the gecit package feed
echo "src/gz gecit https://aliyusufergin.github.io/gecit/opkg" > /opt/etc/opkg/gecit.conf

# 3. Install
opkg update
opkg install gecit
```

Installing starts gecit right away, and it starts by itself at every boot. To see it working:

```sh
/opt/etc/init.d/S51gecit check
```

Blocked sites should show `direct: blocked …` and `with gecit: ok` (see [Check command](#check-command)). Then try Discord on your phone.

## Configuration

The configuration file is `/opt/etc/gecit/gecit.conf`. After changing it:

```sh
/opt/etc/init.d/S51gecit restart
```

Changes to list files need no restart; gecit reads them again within seconds.

| Setting | Default | Meaning |
|---|---|---|
| `ENABLED` | `yes` | `no`: installed but off (does not start at boot or on upgrade either). |
| `MODE` | `auto` | `list`, `auto` or `all`; see [List modes](#list-modes). |
| `LISTS` | `/opt/etc/gecit/lists` | Domain lists. A directory means all `*.txt` files in it. |
| `EXCLUDE` | `/opt/etc/gecit/exclude.txt` | Domains never touched, in any mode. |
| `AUTO_LIST` | `/opt/etc/gecit/auto.txt` | Domains learned in `auto` mode. |
| `HTTP` | `yes` | Also handle plain HTTP (port 80) requests. |
| `TTL` | `3` | TTL of the fake packet as it leaves the router. On Kablonet 2 and 3 work. Lowered automatically for closer servers. |
| `FAKE_HOST` | `www.google.com` | The site name in the fake packet. |
| `WAN` | `auto` | Internet interface. `auto`: the default route's interface (`ppp0` on PPPoE). Several names can be given, separated by spaces. |
| `QUEUE`, `MARK`, `FORCE_MARK` | `200`, `0x40000000`, `0x20000000` | Plumbing; change only if they clash with another tool. |

**Lists.** One domain per line; a domain also covers its subdomains (`discord.com` → `cdn.discord.com`). Lines starting with `#` are comments.

- `lists/default.txt`: the list shipped with gecit (Discord and known blocked sites). **It is replaced on upgrade**; don't edit it.
- For your own domains, create a new file in `lists/`, e.g. `lists/user.txt`. Upgrades leave it alone.
- `exclude.txt`: domains gecit must never touch (if a site breaks with gecit, put it here).
- `auto.txt`: learned domains. To forget one, delete its line.

## List modes

| Mode | Which connections |
|---|---|
| `list` | Only domains in the `LISTS` files. |
| `auto` (default) | `LISTS` plus what gecit learned. The first connection to a blocked site is blocked; gecit notices, adds the site to `auto.txt`, and it opens **when you reload the page**. |
| `all` | All HTTPS/HTTP connections (except EXCLUDE). |

`EXCLUDE` wins in every mode.

gecit counts a site as blocked when:
- the connection is reset well before the server could have answered (the DPI's forged reset: ~1.6 ms on Kablonet, against at least ~9 ms for a real reply from the server), or
- an HTTPS request gets an HTTP redirect instead of TLS (Güvenli İnternet's block page).

Servers' own resets don't count.

## Check command

```sh
/opt/etc/init.d/S51gecit check                   # every listed domain
/opt/etc/init.d/S51gecit check example.org …     # the domains you give
```

For each domain it makes two attempts from the router: **direct** (without gecit) and **with gecit** (even if not listed). A control domain (`example.com`) comes last.

| Result | Meaning |
|---|---|
| `ok` | The server answered. |
| `blocked (reset)` | The connection was reset (DPI). |
| `blocked (block page)` | Redirected to a block page (e.g. Güvenli İnternet). |
| `blocked (closed)` / `blocked (no answer)` | The connection was closed / nothing came back. |
| `no connection` | Could not connect to the server. Usually DNS is giving a block address; see [Requirements](#requirements) item 2. |
| `(not listed)` / `(excluded)` | gecit would normally leave this domain alone; the `with gecit` column still shows the attempt. |

`direct: blocked`, `with gecit: ok` means the block is there and gecit gets past it.

## Upgrade

```sh
opkg update
opkg upgrade gecit
```

Your configuration, your own lists and what was learned are kept.

> **Don't use** `opkg install --force-reinstall gecit`: opkg treats it as remove-and-install, which deletes `/opt/etc/gecit`.

## Uninstall

```sh
opkg remove --autoremove gecit
```

This stops gecit and removes:
- its rules and the kernel module it loaded;
- the `/opt/etc/gecit` directory (configuration, lists, learned domains);
- the feed file (`/opt/etc/opkg/gecit.conf`);
- the `iptables` package that came with it.

**Back up your own lists first if you want to keep them.**

If you installed `wget-ssl` only for gecit, also run `opkg remove --autoremove wget-ssl`.

gecit never touches the router's DNS settings; if you changed them yourself, they stay as they are.

## Troubleshooting

**Log.** gecit writes to the Keenetic system log (the system log page of the web interface), or on Entware:

```sh
ndmc -c "show log" | grep gecit
```

**Status.**

```sh
/opt/etc/init.d/S51gecit status
iptables -t mangle -S | grep gecit     # the rules (iptables comes with gecit)
```

**A site doesn't open.**
1. Run `S51gecit check site.com`.
   - `direct: ok`: the site isn't blocked; the problem is elsewhere.
   - `no connection`: a DNS problem (item 2 of [Requirements](#requirements)).
   - `with gecit: blocked …`: gecit can't get past this block. Set `TTL` to 2, 4 or 5, `restart`, and `check` again. If nothing changes, the ISP may use a different method; please open an issue.
2. In `auto` mode, reload the page once; the first attempt is for learning. You can also add the site to `lists/user.txt` yourself.

**A site breaks with gecit.** Put the domain in `exclude.txt`.

**Turning gecit off for a while.** `/opt/etc/init.d/S51gecit stop` (it starts again at boot). To keep it off, set `ENABLED=no`.

**Configuration error.** `gecit validate` names the line:

```sh
/opt/sbin/gecit validate
```

## ISPs

| ISP | Status | Notes |
|---|---|---|
| Türksat Kablonet | **Tested** | Blocks: resets and Güvenli İnternet redirects. `TTL=3` (2 works too). |
| Türk Telekom | Untested | User reports say it resets like Kablonet. Try the defaults first, then `TTL` 2–5. |
| Vodafone | Untested | User reports say it resets. Try the defaults, then `TTL` 2–5. |
| Superonline | Untested | Reported to use a different DPI box; may not work. |
| TurkNet | Untested | Reported blocks are DNS-only; encrypted DNS alone may be enough. |

If you have tried it on your ISP, please share the result (the `check` output and the `TTL` you used) in an issue, so the table can be updated.

## Models

Look at the output of `opkg print-architecture`:

| Entware architecture | Status |
|---|---|
| `aarch64-3.10` | Tested (Keenetic Hopper KN-3811, KeeneticOS 5.1). |
| `mipsel-3.4` | The package builds; **untested**. |

The kernel modules needed (`nfnetlink_queue`, `xt_NFQUEUE`, `xt_u32`) come with the Netfilter component.

## Limits

- Sites **blocked by IP address** and **bandwidth throttling** cannot be bypassed; gecit is not a VPN.
- gecit does not fix **DNS** blocks; encrypted DNS is required (item 2).
- **IPv4** only. QUIC (HTTP/3) is not handled; Kablonet does not block QUIC.
- When a large ClientHello's site name falls outside its first packet, gecit cannot see the name and sends the fake anyway, so `EXCLUDE` cannot apply to those connections. The fake never reaches the server, so this is harmless.
- Blocked servers closer than ~3 ms to the router can't be learned in `auto` mode; add them to a list yourself.
- Uninstalling deletes everything under `/opt/etc/gecit`.

## Development

```sh
docker/build.sh test                                                  # unit tests
docker/build.sh build --release --target aarch64-unknown-linux-musl   # router binary
tools/mkipk.sh                                                        # dist/gecit_<version>_aarch64-3.10.ipk
```

Layout:

| File | What it does |
|---|---|
| `src/engine.rs` | Packet handling and learning |
| `src/supervise.rs` | Watchdog |
| `src/check.rs` | The check command |
| `files/rules.sh` | Netfilter rules |
| `files/S51gecit` | Service script |
| `files/50-gecit.sh` | NDM hook |

## Licence

MIT or Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
