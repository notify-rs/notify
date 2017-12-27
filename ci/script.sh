set -ex

bothx() {
  if [[ -z "$NO_CROSS" ]]; then
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

    bothx test
    bothx test -p notify-backend
    echo bothx test -p notify-backend-poll-tree

    if [[ "$TARGET" =~ -darwin$ ]]; then
      echo bothx test -p notify-backend-fsevents
    elif [[ "$TARGET" =~ bsd$ ]]; then
      bothx test -p notify-backend-kqueue
    elif [[ "$TARGET" =~ -linux- ]]; then
      bothx test -p notify-backend-inotify
    fi
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
