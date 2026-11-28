# Changelog

## 0.1.6

- `auto` mode: a plain-HTTP answer on a port other than 443 now counts as a block only when it arrives faster than the server could answer, so plain-HTTP servers on other ports are no longer learned by mistake.
- Connections that send no request (for example unanswered SYNs) leave the connection table after 5 s instead of 20 s, so heavy connection churn can't fill it.

## 0.1.5

- Handle TLS ClientHellos on any TCP port, not only 443. Some filters act on other TLS ports too (for example GeForce NOW session traffic on 48322).
- `check`: the "no connection" hint now also covers IP blocks and unreachable hosts.

## 0.1.4

First public release. Tested on Türksat Kablonet with a Keenetic Hopper KN-3811 (aarch64); the mipsel package is untested.

- Fake-ClientHello / fake-HTTP-request bypass with a TTL capped below the server's distance.
- List modes `list`, `auto` (learns blocked domains from forged resets and injected block pages) and `all`, plus an exclude list.
- Watchdog: a hung engine is restarted within about a second; the internet keeps working if the engine fails.
- `keendpi check`: per-domain direct vs. bypass test.
- `VERBOSE=yes` for per-connection logs; missing queue modules are loaded when present.
- opkg package with init script, NDM netfilter hook (rules survive table rewrites and reboots) and clean uninstall.
