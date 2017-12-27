set -ex

bothx() {
  cross $*
  cross $* --release
}

main() {
    bothx build --target $TARGET

    if [ ! -z $DISABLE_TESTS ]; then
        return
    fi

    bothx test --target $TARGET
    bothx test --target $TARGET -p notify-backend
    echo bothx test --target $TARGET -p notify-backend-poll-tree

    if [[ "$TARGET" =~ -darwin$ ]]; then
      echo bothx test --target $TARGET -p notify-backend-fsevents
    elif [[ "$TARGET" =~ -(\w+bsd|ios)$ ]]; then
      bothx test --target $TARGET -p notify-backend-kqueue
    elif [[ "$TARGET" =~ -linux- ]]; then
      bothx test --target $TARGET -p notify-backend-inotify
    fi
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
