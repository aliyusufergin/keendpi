# Releasing

## One-time setup (public repository)

The opkg feed is served by GitHub Pages from the `gh-pages` branch, which the release job writes.

1. Push one release tag (below), so that `gh-pages` exists.
2. Settings → Pages → Source: **Deploy from a branch**, branch `gh-pages`, folder `/`. With the CLI:

   ```sh
   gh api -X POST repos/OWNER/keendpi/pages -f "source[branch]=gh-pages" -f "source[path]=/"
   ```

3. Check: `https://OWNER.github.io/keendpi/opkg/Packages` lists the packages.

## Each release

1. Set `version` in `Cargo.toml`. The release job refuses a tag that doesn't match it.
2. Add a `## <version>` section to `CHANGELOG.md`. That section becomes the release notes.
3. Commit, then tag and push:

   ```sh
   git tag v0.1.4
   git push origin main v0.1.4
   ```

The `release` job:
- creates or updates the GitHub release with the `.ipk` files;
- rebuilds `gh-pages/opkg` (`Packages`, `Packages.gz` and the packages) from them.

The job can be re-run. A failed untested build (mipsel) does not block the release; its package is then missing from the release and the feed.
