#!/bin/sh
# Checks that the package versions of FORMAT (deb, rpm or arch) sort as SemVer does, so that a
# pre-release such as 1.0.0-rc.1 comes before its release, as CI runs it.
#
# Maps ascending SemVer versions with `packaging/update.py package-version` and compares each
# with the next by the version comparison of the package manager (`dpkg --compare-versions`,
# `rpm.vercmp`, `vercmp`), in a fresh container of IMAGE.
#
# Usage: scripts/package-version-order.sh FORMAT IMAGE
set -eu

format="${1:?usage: scripts/package-version-order.sh FORMAT IMAGE}"
image="${2:?usage: scripts/package-version-order.sh FORMAT IMAGE}"

if [ "$image" != --compare ]; then
    root="$(cd "$(dirname "$0")/.." && pwd)"
    map="$root/packaging/update.py"
    versions=""
    for version in 1.0.0-alpha 1.0.0-alpha.1 1.0.0-beta 1.0.0-beta.2 1.0.0-rc.1 1.0.0-rc.2 \
        1.0.0-rc.10 1.0.0 1.0.1-rc.1 1.0.1 1.10.0-rc.1 1.10.0; do
        versions="$versions $(python3 "$map" package-version "$format" "$version")"
    done
    # The versions hold no whitespace, so word splitting passes one argument per version
    # shellcheck disable=SC2086
    exec docker run --rm --volume "$root/scripts/package-version-order.sh:/order.sh:ro" \
        "$image" sh /order.sh "$format" --compare $versions
fi

shift 2
previous="$1"
shift
for version in "$@"; do
    case "$format" in
        deb) dpkg --compare-versions "$previous" lt "$version" ;;
        rpm) test "$(rpm --eval "%{lua: print(rpm.vercmp('$previous', '$version'))}")" = -1 ;;
        arch) test "$(vercmp "$previous" "$version")" = -1 ;;
        *)
            echo "error: unknown package format $format" >&2
            exit 1
            ;;
    esac || {
        echo "error: $format sorts $previous after $version" >&2
        exit 1
    }
    previous="$version"
done
echo "$format versions sort as SemVer, up to $previous"
