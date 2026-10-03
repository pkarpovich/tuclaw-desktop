#!/usr/bin/env fish

set -l min_macos 14.0
set -l install no
set -l open no
for arg in $argv
    switch $arg
        case --install
            set install yes
        case --open
            set open yes
        case '*'
            echo "usage: bundle-mac.fish [--install] [--open]" >&2
            exit 2
    end
end

set -l root (path resolve (status dirname)/..)
cd $root; or exit 1

set -l app_version (string match -rg '^version = "(.+)"' < Cargo.toml)[1]
set -l build (git rev-list --count HEAD)
set -l commit (git rev-parse --short=7 HEAD)
if test -z "$app_version" -o -z "$build" -o -z "$commit"
    echo "cannot read the version, build number or commit" >&2
    exit 1
end

set -x TUCLAW_COMMIT $commit
set -x MACOSX_DEPLOYMENT_TARGET $min_macos
cargo build --release -p tuclaw-desktop; or exit 1

set -l app target/release/bundle/Tuclaw.app
rm -rf $app
mkdir -p $app/Contents/MacOS $app/Contents/Resources; or exit 1
cp target/release/tuclaw-desktop $app/Contents/MacOS/tuclaw-desktop; or exit 1

sed -e "s/@VERSION@/$app_version/" -e "s/@BUILD@/$build/" -e "s/@COMMIT@/$commit/" -e "s/@MIN_MACOS@/$min_macos/" \
    app/resources/Info.plist > $app/Contents/Info.plist; or exit 1
plutil -lint -s $app/Contents/Info.plist; or exit 1

set -l partial (mktemp -t tuclaw-icon)
xcrun actool app/resources/AppIcon.icon --compile $app/Contents/Resources \
    --platform macosx --minimum-deployment-target $min_macos \
    --app-icon AppIcon --output-partial-info-plist $partial >/dev/null; or exit 1
rm -f $partial

codesign --force --sign - $app; or exit 1
codesign --verify --strict $app; or exit 1
echo "built $app ($app_version, build $build, $commit)"

if test $install = yes
    rm -rf /Applications/Tuclaw.app
    ditto $app /Applications/Tuclaw.app; or exit 1
    echo "installed /Applications/Tuclaw.app"
end

if test $open = yes
    set -l target $app
    if test $install = yes
        set target /Applications/Tuclaw.app
    end
    pkill -f 'Tuclaw.app/Contents/MacOS/tuclaw-desktop'
    for attempt in (seq 50)
        set -l running (pgrep -f 'Tuclaw.app/Contents/MacOS/tuclaw-desktop')
        set -l registered (lsappinfo find bundleid=dev.pkarpovich.tuclaw)
        test -z "$running" -a -z "$registered"; and break
        sleep 0.1
    end
    for attempt in (seq 3)
        if open $target
            echo "opened $target"
            exit 0
        end
        sleep 0.5
    end
    echo "could not open $target" >&2
    exit 1
end
