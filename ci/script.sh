set -ex

bothx() {
  if [ -z $NO_CROSS ]; then
    cross $* --target $TARGET
    cross $* --target $TARGET --release
  else
    cargo $*
    cargo $* --release
  fi
}

main() {
    bothx build

    if [ ! -z $DISABLE_TESTS ]; then
        return
    fi

    if [ ! -z $NO_CROSS ]; then
      if [[ "$TRAVIS_OS_NAME" = "linux" ]]; then
        TARGET="x86_64-unknown-linux-gnu"
      elif [[ "$TRAVIS_OS_NAME" = "osx" ]]; then
        TARGET="x86_64-apple-darwin"
      fi
    fi

    bothx test
    bothx test -p notify-backend
    echo bothx test -p notify-backend-poll-tree

    if [[ "$TARGET" =~ -darwin$ ]]; then
      echo bothx test -p notify-backend-fsevent
      bothx test -p notify-backend-fsevent
    elif [[ "$TARGET" =~ -linux- ]]; then
      bothx test -p notify-backend-inotify
    fi
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
