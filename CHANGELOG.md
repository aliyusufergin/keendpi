# Changelog

## 0.1.3

First public release. Tested on Türksat Kablonet with a Keenetic Hopper KN-3811 (aarch64); the mipsel package is untested.

- Fake-ClientHello / fake-HTTP-request bypass with a TTL capped below the server's distance.
- List modes `list`, `auto` (learns blocked domains from forged resets and injected block pages) and `all`, plus an exclude list.
- Watchdog: a hung engine is restarted within about a second; the internet keeps working if the engine fails.
- `gecit check`: per-domain direct vs. bypass test.
- opkg package with init script, NDM netfilter hook (rules survive table rewrites and reboots) and clean uninstall.
