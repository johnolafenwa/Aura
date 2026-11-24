#!/bin/sh
set -eu

release_tag=v0.17.0-preview
prefix=${AURA_INSTALL_PREFIX:-"$HOME/.local"}
base_url=${AURA_INSTALL_BASE_URL:-"https://github.com/johnolafenwa/Aura/releases/download/$release_tag"}

fail() {
  printf 'Aura installer: %s\n' "$1" >&2
  exit 1
}

# Everything runs from main, called on the last line, so a download cut off
# partway through `curl ... | sh` defines code but runs none of it.
# Checks one file of the release against SHA256SUMS, both in the current
# directory.
verify() {
  awk -v name="$1" '
    { sub(/\r$/, "") }
    NF == 2 && $1 ~ /^[0-9a-fA-F]{64}$/ {
      file = $2
      sub(/^\*/, "", file)
      if (file == name) {
        print $1 "  " file
        count++
      }
    }
    END {
      if (count != 1) exit 1
    }
  ' SHA256SUMS > checksum-entry ||
    fail "$1 is missing from SHA256SUMS"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c checksum-entry >/dev/null || fail "checksum verification failed"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c checksum-entry >/dev/null || fail "checksum verification failed"
  else
    fail "sha256sum or shasum is required to verify the release"
  fi
}

# Pre-scans a tar.gz archive so only regular files and directories under the
# expected root are extracted, into an isolated directory separate from
# SHA256SUMS.
safe_extract() {
  archive_file=$1
  expected_root=$2
  extract_dir=$3
  tar -tvzf "$archive_file" | awk '
    /^[d-]/ { count++; next }
    { exit 1 }
    END { if (count == 0) exit 1 }
  ' || fail "$archive_file contains unsupported non-regular entries"
  tar -tzf "$archive_file" | awk -v root="$expected_root" '
    {
      p = $0
      if (p == "" || substr(p, 1, 1) == "/") exit 1
      n = split(p, parts, "/")
      for (i = 1; i <= n; i++) {
        if (parts[i] == "..") exit 1
      }
      if (p != root && p != root "/" && substr(p, 1, length(root) + 1) != root "/") exit 1
      count++
    }
    END {
      if (count == 0) exit 1
    }
  ' || fail "$archive_file contains invalid archive paths"
  rm -rf "$extract_dir"
  mkdir -p "$extract_dir"
  tar -xzf "$archive_file" -C "$extract_dir" || fail "could not extract $archive_file"
}

main() {
  # `--target <name>` also installs what `aura build --target <name>`
  # links: sh -s -- --target x86_64-unknown-linux-musl
  build_targets=
  while test $# -gt 0; do
    case "$1" in
      --target)
        test $# -ge 2 || fail "--target needs a target name"
        build_targets="$build_targets $2"
        shift 2
        ;;
      *)
        fail "unknown option '$1'; the only option is --target <name>"
        ;;
    esac
  done

  command -v curl >/dev/null 2>&1 || fail "curl is required"
  command -v tar >/dev/null 2>&1 || fail "tar is required"

  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64|Darwin-aarch64)
      target=aarch64-apple-darwin
      ;;
    Darwin-x86_64)
      target=x86_64-apple-darwin
      ;;
    Linux-x86_64|Linux-amd64)
      target=x86_64-unknown-linux-gnu
      ;;
    Linux-aarch64|Linux-arm64)
      target=aarch64-unknown-linux-gnu
      ;;
    *)
      fail "no release archive is available for $(uname -s) $(uname -m)"
      ;;
  esac

  archive_root="aura-$release_tag-$target"
  archive="$archive_root.tar.gz"
  tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/aura-install.XXXXXX") || fail "could not create a temporary directory"
  trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM

  case "$base_url" in
    https://*)
      set -- --proto '=https' --tlsv1.2
      ;;
    file://*)
      set -- --proto '=file'
      ;;
    *)
      fail "refusing insecure AURA_INSTALL_BASE_URL '$base_url'; expected an https:// URL"
      ;;
  esac

  printf 'Downloading Aura %s for %s...\n' "$release_tag" "$target"
  curl -fsSL "$@" -o "$tmpdir/$archive" -- "$base_url/$archive" || fail "could not download $archive"
  curl -fsSL "$@" -o "$tmpdir/SHA256SUMS" -- "$base_url/SHA256SUMS" || fail "could not download SHA256SUMS"

  (cd "$tmpdir" && verify "$archive") || exit 1

  host_extract="$tmpdir/extract-host"
  safe_extract "$tmpdir/$archive" "$archive_root" "$host_extract"
  source_root="$host_extract/$archive_root"
  test -x "$source_root/bin/aura" || fail "the archive does not contain bin/aura"
  test -f "$source_root/lib/aura/libaura_runtime.a" || fail "the archive is missing the native runtime"
  test -f "$source_root/lib/aura/native-link-args.json" || fail "the archive is missing native-link-args.json"

  mkdir -p "$prefix/bin" "$prefix/lib/aura" "$prefix/share/aura"
  rm -f "$prefix/bin/aura"
  cp "$source_root/bin/aura" "$prefix/bin/aura"
  chmod 755 "$prefix/bin/aura"
  rm -f "$prefix/lib/aura/libaura_runtime.a"
  cp "$source_root/lib/aura/libaura_runtime.a" "$prefix/lib/aura/libaura_runtime.a"
  # Earlier releases shipped the runtime as libaura_compiler.a.
  rm -f "$prefix/lib/aura/libaura_compiler.a"
  rm -f "$prefix/lib/aura/native-link-args.json"
  cp "$source_root/lib/aura/native-link-args.json" "$prefix/lib/aura/native-link-args.json"

  # A target pack holds a runtime library of one release, so the packs
  # already under the prefix are replaced with this release's.
  for directory in "$prefix"/lib/aura/targets/*; do
    test -d "$directory" || continue
    build_targets="$build_targets ${directory##*/}"
  done
  case "$build_targets-$target" in
    -*|*-linux-gnu) ;;
    *) fail "--target is for Linux installations: aura build --target runs on Linux" ;;
  esac
  installed_targets=
  set -f
  for build_target in $build_targets; do
    case " $installed_targets " in
      *" $build_target "*) continue ;;
    esac
    case "$build_target" in
      x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
      *) set +f; fail "no target pack is available for '$build_target'" ;;
    esac
    test "$build_target" != "$target" || continue
    pack_root="aura-$release_tag-target-$build_target"
    pack="$pack_root.tar.gz"
    printf 'Downloading the target pack for %s...\n' "$build_target"
    curl -fsSL "$@" -o "$tmpdir/$pack" -- "$base_url/$pack" || fail "could not download $pack"
    (cd "$tmpdir" && verify "$pack") || exit 1
    pack_extract="$tmpdir/extract-$build_target"
    safe_extract "$tmpdir/$pack" "$pack_root" "$pack_extract"
    pack_source="$pack_extract/$pack_root/lib/aura/targets/$build_target"
    test -f "$pack_source/libaura_runtime.a" || fail "$pack does not contain the target's runtime"
    mkdir -p "$prefix/lib/aura/targets"
    rm -rf "$prefix/lib/aura/targets/$build_target"
    cp -R "$pack_source" "$prefix/lib/aura/targets/$build_target"
    installed_targets="$installed_targets $build_target"
  done
  set +f

  if test -d "$source_root/examples"; then
    rm -rf "$prefix/share/aura/examples"
    cp -R "$source_root/examples" "$prefix/share/aura/examples"
  fi
  for file in README.md AURA_CLI_README.md LICENSE; do
    if test -f "$source_root/$file"; then
      rm -f "$prefix/share/aura/$file"
      cp "$source_root/$file" "$prefix/share/aura/$file"
    fi
  done

  printf '\nAura is installed at %s/bin/aura\n' "$prefix"
  case ":$PATH:" in
    *":$prefix/bin:"*)
      printf 'Run: aura --version\n'
      ;;
    *)
      printf 'Add Aura to this shell with:\n  export PATH="%s/bin:$PATH"\n' "$prefix"
      ;;
  esac
}

main "$@"
