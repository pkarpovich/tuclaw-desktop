#!/usr/bin/env fish

set -l profile debug
set -l run no
for arg in $argv
    switch $arg
        case --release
            set profile release
        case --run
            set run yes
        case '*'
            echo "usage: ios.fish [--release] [--run]" >&2
            exit 2
    end
end

set -l root (path resolve (status dirname)/..)
cd $root; or exit 1

set -l target aarch64-apple-ios-sim
set -l sim_name "Tuclaw iPhone 17 Pro"
set -l sim_type com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro
set -l sim_runtime com.apple.CoreSimulator.SimRuntime.iOS-26-5
set -l bundle_id dev.pkarpovich.tuclaw.ios

set -l cargo_flags --lib --target $target -p tuclaw-ios
set -l configuration Debug
if test $profile = release
    set -a cargo_flags --release
    set configuration Release
end
set -x TUCLAW_COMMIT (git rev-parse --short=7 HEAD)
cargo build $cargo_flags; or exit 1

set -l out $root/target/ios
mkdir -p $out
xcodegen generate --quiet --spec ios/xcode/project.yml; or exit 1

set -l app_version (string match -rg '^version = "(.+)"' < Cargo.toml)[1]
set -l build (git rev-list --count HEAD)
xcodebuild -quiet -project ios/xcode/Tuclaw.xcodeproj -scheme Tuclaw -configuration $configuration \
    -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' -derivedDataPath $out/build \
    TUCLAW_RUST_LIB=$root/target/$target/$profile/libtuclaw_ios.a \
    TUCLAW_VERSION=$app_version TUCLAW_BUILD=$build build; or exit 1

set -l app $out/build/Build/Products/$configuration-iphonesimulator/Tuclaw.app
echo "built $app"
if test $run = no
    exit 0
end

set -l udid (xcrun simctl list devices -j | python3 -I -c '
import json, sys
name, runtime = sys.argv[1], sys.argv[2]
for device in json.load(sys.stdin)["devices"].get(runtime, []):
    if device["name"] == name and device["isAvailable"]:
        print(device["udid"])
        break
' $sim_name $sim_runtime)
if test -z "$udid"
    set udid (xcrun simctl create $sim_name $sim_type $sim_runtime); or exit 1
end
xcrun simctl boot $udid 2>/dev/null
xcrun simctl bootstatus $udid -b >/dev/null; or exit 1
open -a Simulator --args -CurrentDeviceUDID $udid
xcrun simctl install $udid $app; or exit 1
set -l log $out/console.log
xcrun simctl launch --terminate-running-process --stdout=$log --stderr=$log $udid $bundle_id; or exit 1
echo "console: $log"
echo "launched on $sim_name ($udid)"
