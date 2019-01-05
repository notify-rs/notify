set -ex

bothx() {
  if [ -z $NO_CROSS ]; then
    cross $* --target $TARGET
  else
    cargo $*
  fi
}

main() {
  if [ ! -z $DISABLE_TESTS ]; then
      bothx build
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
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
