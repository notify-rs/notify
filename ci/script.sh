set -ex

bothx() {
  OPTS="$* --target $TARGET"
  if [ ! -z $TRAVIS_TAG ]; then
    OPTS="$OPTS --release"
  fi

  if [ -z $NO_CROSS ]; then
    cross $OPTS
  else
    cargo $OPTS
  fi
}

if [ ! -z $DISABLE_TESTS ]; then
    bothx build
else
  if [ ! -z $NO_CROSS ]; then
    if [[ "$TRAVIS_OS_NAME" = "linux" ]]; then
      TARGET="x86_64-unknown-linux-gnu"
    elif [[ "$TRAVIS_OS_NAME" = "osx" ]]; then
      TARGET="x86_64-apple-darwin"
    elif [[ "$TRAVIS_OS_NAME" = "windows" ]]; then
      TARGET="x86_64-pc-windows-msvc"
    fi
  fi

  bothx test
fi
