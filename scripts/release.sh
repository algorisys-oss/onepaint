#!/usr/bin/env bash
# Release onepaint: tag main and let GitHub Actions build and publish it.
#
#   scripts/release.sh               release the current version (Cargo.toml)
#   scripts/release.sh 0.2.0         bump to 0.2.0, commit, then release it
#   scripts/release.sh 0.2.0 --dry-run   say what would happen, change nothing
#   scripts/release.sh ... --yes     skip the confirmation prompt
#   scripts/release.sh ... --watch   follow the build until it finishes (needs gh)
#
# Pushing the tag v<version> starts .github/workflows/build.yml, which builds
# Linux, Windows and macOS and publishes a GitHub Release with the files.
# Nothing is pushed until you confirm.
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "release: $*" >&2; exit 1; }

version="" dry=0 yes=0 watch=0
for a in "$@"; do
  case "$a" in
    --dry-run) dry=1 ;;
    --yes|-y) yes=1 ;;
    --watch) watch=1 ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) die "unknown option $a" ;;
    *) [[ -z "$version" ]] || die "one version only"; version="${a#v}" ;;
  esac
done

current=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[[ -n "$current" ]] || die "no [workspace.package] version in Cargo.toml"
version=${version:-$current}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "not a version: $version (want X.Y.Z)"
tag="v$version"

# Release only a clean main that matches GitHub.
[[ "$(git rev-parse --abbrev-ref HEAD)" == main ]] || die "not on main"
[[ -z "$(git status --porcelain)" ]] || die "the working tree has changes; commit or stash them first"
git fetch -q origin main --tags
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] || die "main differs from origin/main; pull or push first"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && die "tag $tag already exists"
git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1 && die "tag $tag already exists on GitHub"

echo "Release $tag from main at $(git rev-parse --short HEAD)"
[[ "$version" != "$current" ]] && echo "  bump the version $current -> $version and commit it"
echo "  tag $tag and push it: GitHub Actions builds Linux, Windows and macOS and publishes the release"
if (( dry )); then echo "(dry run: nothing changed)"; exit 0; fi
if (( ! yes )); then
  read -r -p "Go ahead? [y/N] " ok
  [[ "$ok" == [yY]* ]] || die "cancelled"
fi

if [[ "$version" != "$current" ]]; then
  sed -i.bak "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$version\"/" Cargo.toml && rm -f Cargo.toml.bak
  cargo update --workspace --quiet
  git add Cargo.toml Cargo.lock
  git commit -q -m "Release $tag"
  git push -q origin main
fi
git tag -a "$tag" -m "onepaint $version"
git push -q origin "$tag"
echo "Pushed $tag."

if command -v gh >/dev/null; then
  repo=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)
  [[ -n "$repo" ]] && echo "Build: https://github.com/$repo/actions/workflows/build.yml  Release: https://github.com/$repo/releases/tag/$tag"
  if (( watch )); then
    sleep 5
    run=$(gh run list --workflow build.yml --branch "$tag" --limit 1 --json databaseId -q '.[0].databaseId' 2>/dev/null || true)
    [[ -n "$run" ]] && gh run watch "$run" --exit-status
  fi
fi
