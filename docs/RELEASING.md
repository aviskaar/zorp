# Releasing zorp

This document records the release process and the download-count reading rules
that travel with every published number.

## Release process

Releases are tag-driven. Pushing a tag `v*` triggers the `Release` workflow,
which builds prebuilt binaries for all supported targets, packages them with
checksums, and publishes a GitHub Release using `softprops/action-gh-release@v2`.
The `Desktop Release` workflow builds the universal macOS DMG and uploads it to
the same tag with `--clobber`, so a DMG can be re-uploaded after a signing or
notarisation fix.

A manual `workflow_dispatch` run builds and uploads artifacts without publishing,
which is how you check the pipeline before relying on it.

The `default-version` job refuses a tag whose workspace version or Dockerfile
default disagrees. The check lives in `scripts/check-release-version.sh` so it
runs on every pull request, not only during a release.

## Reading download counts

GitHub counts every asset download already. Each release asset carries
`download_count`, cumulative since upload, readable without a token on a public
repository:

```sh
gh api repos/aviskaar/zorp/releases \
  --jq '.[] | .tag_name as $t | .assets[] | "\($t)\t\(.name)\t\(.download_count)"'
```

Three things about that number matter more than the number.

- **It is cumulative and has no history.** GitHub keeps one integer per asset.
  Nothing tells you when the downloads happened or whether they are still
  happening.

- **`gh release upload --clobber` resets it to zero.** `desktop.yml` uses
  `--clobber` so a DMG can be re-uploaded after a signing or notarisation fix.
  Every such re-upload is a new asset with a fresh count, and the old count is
  gone. This is the strongest reason to keep a snapshot outside GitHub.

- **It counts every fetch, not every person.** `curl`, CI, a mirror, a
  crawler, and `install.sh` all count once each. The `.sha256` sibling is the
  tell: `install.sh` fetches the tarball and its checksum together, so a
  tarball whose checksum count matches was installed by script, and a DMG with
  a zero checksum count was downloaded by a person in a browser. Read the
  pairs, not the singles.

**What is not the answer.** The repository's Insights, Traffic page shows
visitors and clones over fourteen days and says nothing about release assets.
Cloudflare's dashboard for the `zorp-ui` Worker counts requests for the chat
UI, which is a different thing from the DMG.

## Reading rules

Wherever the count is printed, the `.sha256` pairing and the `--clobber` reset
are one line beside it, or somebody will read "2" as two people and "0 after
re-upload" as nobody.

- Pair each asset with its `.sha256` sibling. Matching counts means script
  installs; a DMG whose checksum count is zero means human browser downloads.
- Treat every `--clobber` re-upload as a new asset. The old count is gone.
  The history workflow (`.github/workflows/downloads.yml`) snapshots daily so
  the pre-re-upload number is preserved.
- The shields.io badges on the README show total and latest-release totals.
  They are convenient but opaque. The per-asset history lives in the `stats`
  branch (`downloads.jsonl`) and is the source of truth for any future page.