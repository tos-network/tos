#!/bin/bash

# TOS Network - Cross-platform build script
# support: ARM64, ARMv7, x86_64 linux, Windows x86_64, macOS
targets=("aarch64-unknown-linux-gnu" "armv7-unknown-linux-gnueabihf" "x86_64-unknown-linux-musl" "x86_64-unknown-linux-gnu" "x86_64-pc-windows-gnu" "x86_64-apple-darwin" "aarch64-apple-darwin")
binaries=("tos_daemon" "tos_miner" "tos_wallet" "tos_genesis" "tos_ai_miner")
extra_files=("README.md" "BOOTSTRAP_NODE_DEPLOYMENT.md" "LICENSE")

# verify that we have cross installed
if ! command -v cross &> /dev/null
then
    echo "cross could not be found, please install it for cross compilation"
    exit
fi

# Cross needs docker to be running (Linux only)
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    echo "Starting docker daemon (Linux)"
    sudo systemctl start docker
elif [[ "$OSTYPE" == "darwin"* ]]; then
    echo "Make sure Docker Desktop is running on macOS"
    # Check if docker is running
    if ! docker info >/dev/null 2>&1; then
        echo "Docker is not running. Please start Docker Desktop."
        exit 1
    fi
fi

echo "Updating using rustup"
rustup update stable

echo "Only build in stable"
rustup default stable

echo "Deleting build folder"
rm -rf build

# Create build directory
mkdir -p build

# compile all binaries for all targets
echo "Compiling TOS Network binaries for all targets"
for target in "${targets[@]}"; do
    echo "Building for target: $target"

    # support the target to build it
    rustup target add $target

    # Use native cargo for local builds, cross for cross-compilation
    if [[ "$target" == "x86_64-apple-darwin" && "$OSTYPE" == "darwin"* ]]; then
        echo "Using native cargo for $target"
        cargo build --target $target --release
        build_dir="target/$target/release"
    elif [[ "$target" == "aarch64-apple-darwin" && "$OSTYPE" == "darwin"* ]]; then
        echo "Using native cargo for $target"
        cargo build --target $target --release
        build_dir="target/$target/release"
    else
        echo "Using cross for $target"
        cross clean
        cross build --target $target --release
        build_dir="target/$target/release"
    fi

    mkdir -p build/$target
    # copy generated binaries to build directory
    for binary in "${binaries[@]}"; do
        # add .exe extension to windows binaries
        binary_name="$binary"
        if [[ "$target" == *"windows"* ]]; then
            binary_name="$binary.exe"
        fi

        # Check if binary exists before copying
        if [[ -f "$build_dir/$binary_name" ]]; then
            cp "$build_dir/$binary_name" "build/$target/$binary_name"
            echo "  ✓ Copied $binary_name"
        else
            echo "  ⚠ Warning: $binary_name not found in $build_dir"
        fi
    done

    # copy extra files
    for file in "${extra_files[@]}"; do
        if [[ -f "$file" ]]; then
            cp "$file" "build/$target/$file"
            echo "  ✓ Copied $file"
        else
            echo "  ⚠ Warning: $file not found"
        fi
    done

    echo "✓ Build completed for $target"
    echo ""
done

echo "Creating archives for all targets"
for target in "${targets[@]}"; do
    # Skip if build directory doesn't exist or is empty
    if [[ ! -d "build/$target" ]] || [[ -z "$(ls -A "build/$target" 2>/dev/null)" ]]; then
        echo "⚠ Skipping $target - no build artifacts found"
        continue
    fi

    # generate checksums
    echo "Generating checksums for $target"
    cd "build/$target"
    > checksums.txt
    for binary in "${binaries[@]}"; do
        # add .exe extension to windows binaries
        binary_name="$binary"
        if [[ "$target" == *"windows"* ]]; then
            binary_name="$binary.exe"
        fi

        if [[ -f "$binary_name" ]]; then
            sha256sum "$binary_name" >> checksums.txt
        fi
    done
    cd ../..

    # create archive
    cd build/
    archive_name=""
    if [[ "$target" == *"windows"* ]]; then
        archive_name="tos-network-$target.zip"
        zip -r "$archive_name" "$target"
    elif [[ "$target" == *"darwin"* ]]; then
        archive_name="tos-network-$target.tar.gz"
        tar -czf "$archive_name" "$target"
    else
        archive_name="tos-network-$target.tar.gz"
        tar -czf "$archive_name" "$target"
    fi
    echo "✓ Created archive: $archive_name"
    cd ..
done

# Generate final checksums.txt in build/
echo "Generating final release checksums"
cd build/
> release-checksums.txt

# Count successful builds
successful_builds=0
total_targets=${#targets[@]}

for target in "${targets[@]}"; do
    archive_name=""
    if [[ "$target" == *"windows"* ]]; then
        archive_name="tos-network-$target.zip"
    else
        archive_name="tos-network-$target.tar.gz"
    fi

    if [[ -f "$archive_name" ]]; then
        sha256sum "$archive_name" >> release-checksums.txt
        ((successful_builds++))
    fi
done
cd ..

echo ""
echo "🎉 TOS Network Build Summary:"
echo "   ✓ Successfully built for $successful_builds/$total_targets targets"
echo "   📦 Archives created in build/ directory"
echo "   🔐 Checksums available in build/release-checksums.txt"
echo ""
echo "Build completed! 🚀"