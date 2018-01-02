#!/bin/sh

set -ex

date=$(date +'%Y-%m-%d')
LOGDIR="cross-logs/$date"
mkdir -p "$LOGDIR"

buildx() {
  cross build --target $*
  cross build --release --target $*
}

testx() {
  cross test --target $*
  cross test --release --target $*
}

logfile() {
  echo "$LOGDIR/$2.$3.$1.log"
}

target() {
  check="${1}x"
  target="$2"

  $check $target \
    2>&1 | tee $(logfile $check $target notify)

  $check $target -p notify-backend \
    2>&1 | tee $(logfile $check $target notify-backend)

  #$check $target -p notify-backend-poll-tree \
  #  2>&1 | tee $(logfile $check $target notify-backend-poll-tree)

  if [[ "$target" =~ -linux- ]]; then
    $check $target -p notify-backend-inotify \
      2>&1 | tee $(logfile $check $target notify-backend-inotify)
  elif [[ "$target" =~ -darwin$ ]]; then
    echo not ready yet
    $check $target -p notify-backend-fsevent \
      2>&1 | tee $(logfile $check $target notify-backend-fsevent)
  elif [[ "$target" =~ bsd ]]; then
    $check $target -p notify-backend-kqueue \
      2>&1 | tee $(logfile $check $target notify-backend-kqueue)
  fi
}

packlogs() {
  set +ex
  tar czf "${LOGDIR}.tgz" "$LOGDIR"
  echo
  echo "===== ${LOGDIR}.tgz ====="
  echo
}

main() {
  target build x86_64-unknown-freebsd
  target build i686-unknown-freebsd

  target test x86_64-linux-android
  target test i686-unknown-linux-gnu
  target test x86_64-unknown-linux-musl

  packlogs
}

extra() {
  target build x86_64-unknown-netbsd
  target build x86_64-sun-solaris
  target build sparcv9-sun-solaris
  target build s390x-unknown-linux-gnu

  target test aarch64-linux-android
  target test aarch64-unknown-linux-gnu
  target test arm-linux-androideabi
  target test arm-unknown-linux-gnueabi
  target test arm-unknown-linux-musleabi
  target test armv7-linux-androideabi
  target test armv7-unknown-linux-gnueabihf
  target test armv7-unknown-linux-musleabihf
  target test i586-unknown-linux-gnu
  target test i686-linux-android
  target test i686-pc-windows-gnu
  target test i686-unknown-linux-musl
  target test mips-unknown-linux-gnu
  target test mips64-unknown-linux-gnuabi64
  target test mips64el-unknown-linux-gnuabi64
  target test mipsel-unknown-linux-gnu
  target test powerpc-unknown-linux-gnu
  target test powerpc64-unknown-linux-gnu
  target test powerpc64le-unknown-linux-gnu
  target test sparc64-unknown-linux-gnu
  target test x86_64-pc-windows-gnu
  target test x86_64-unknown-linux-gnu

  # target test x86_64-unknown-dragonfly # no std yet

  packlogs
}

if [[ -z "$1" ]]; then
  main
elif [[ "$1" = "extra" ]]; then
  extra
else
  target $*
  packlogs
fi
