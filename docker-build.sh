#!/bin/bash

# TOS Network - Docker Build Script
# Builds Docker images for TOS Network components

set -e

# Default values
DEFAULT_TAG="latest"
DEFAULT_APP="tos_daemon"
REGISTRY=""
PUSH=false
NO_CACHE=false

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Available apps
AVAILABLE_APPS=("tos_daemon" "tos_miner" "tos_wallet" "tos_genesis")

# Help function
show_help() {
    cat << EOF
TOS Network Docker Build Script

Usage: $0 [OPTIONS]

OPTIONS:
    -a, --app APP           Application to build (default: tos_daemon)
                           Available: ${AVAILABLE_APPS[*]}
    -t, --tag TAG          Docker image tag (default: latest)
    -r, --registry REGISTRY Registry prefix (e.g., ghcr.io/tos-network)
    -p, --push             Push image to registry after build
    --no-cache             Build without using cache
    --all                  Build all applications
    -h, --help             Show this help message

EXAMPLES:
    # Build daemon with default settings
    $0

    # Build specific app with custom tag
    $0 --app tos_miner --tag v1.0.0

    # Build and push to registry
    $0 --app tos_daemon --registry ghcr.io/tos-network --tag v1.0.0 --push

    # Build all applications
    $0 --all --tag v1.0.0

ENVIRONMENT VARIABLES:
    TOS_COMMIT_HASH        Git commit hash (auto-detected if not set)
    DOCKER_BUILDKIT        Enable BuildKit (recommended: 1)

EOF
}

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Get git commit hash
get_commit_hash() {
    if [[ -n "$TOS_COMMIT_HASH" ]]; then
        echo "$TOS_COMMIT_HASH"
    elif git rev-parse --git-dir > /dev/null 2>&1; then
        git rev-parse --short HEAD
    else
        echo "unknown"
    fi
}

# Validate app name
validate_app() {
    local app="$1"
    for valid_app in "${AVAILABLE_APPS[@]}"; do
        if [[ "$app" == "$valid_app" ]]; then
            return 0
        fi
    done
    return 1
}

# Build Docker image
build_image() {
    local app="$1"
    local tag="$2"
    local commit_hash="$3"
    local cache_flag=""

    if [[ "$NO_CACHE" == "true" ]]; then
        cache_flag="--no-cache"
    fi

    local image_name="tos-network-$app"
    if [[ -n "$REGISTRY" ]]; then
        image_name="$REGISTRY/tos-network-$app"
    fi

    local full_tag="$image_name:$tag"

    log_info "Building Docker image for $app"
    log_info "Image: $full_tag"
    log_info "Commit: $commit_hash"

    docker build \
        $cache_flag \
        --build-arg app="$app" \
        --build-arg commit_hash="$commit_hash" \
        -t "$full_tag" \
        -f Dockerfile \
        .

    if [[ $? -eq 0 ]]; then
        log_success "Successfully built $full_tag"

        # Also tag as latest if not already latest
        if [[ "$tag" != "latest" ]]; then
            docker tag "$full_tag" "$image_name:latest"
            log_info "Tagged as $image_name:latest"
        fi

        # Push if requested
        if [[ "$PUSH" == "true" ]]; then
            log_info "Pushing $full_tag to registry..."
            docker push "$full_tag"

            if [[ "$tag" != "latest" ]]; then
                docker push "$image_name:latest"
            fi

            log_success "Successfully pushed $full_tag"
        fi
    else
        log_error "Failed to build $full_tag"
        return 1
    fi
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -a|--app)
            DEFAULT_APP="$2"
            shift 2
            ;;
        -t|--tag)
            DEFAULT_TAG="$2"
            shift 2
            ;;
        -r|--registry)
            REGISTRY="$2"
            shift 2
            ;;
        -p|--push)
            PUSH=true
            shift
            ;;
        --no-cache)
            NO_CACHE=true
            shift
            ;;
        --all)
            BUILD_ALL=true
            shift
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Validate Docker installation
if ! command -v docker &> /dev/null; then
    log_error "Docker is not installed or not in PATH"
    exit 1
fi

# Check if Docker is running
if ! docker info >/dev/null 2>&1; then
    log_error "Docker is not running. Please start Docker."
    exit 1
fi

# Enable BuildKit if not already set
export DOCKER_BUILDKIT=1

# Get commit hash
COMMIT_HASH=$(get_commit_hash)

# Main build logic
if [[ "$BUILD_ALL" == "true" ]]; then
    log_info "Building all TOS Network applications with tag: $DEFAULT_TAG"

    failed_builds=()
    for app in "${AVAILABLE_APPS[@]}"; do
        if ! build_image "$app" "$DEFAULT_TAG" "$COMMIT_HASH"; then
            failed_builds+=("$app")
        fi
        echo ""
    done

    if [[ ${#failed_builds[@]} -eq 0 ]]; then
        log_success "All builds completed successfully! 🚀"
    else
        log_error "Some builds failed: ${failed_builds[*]}"
        exit 1
    fi
else
    # Validate single app
    if ! validate_app "$DEFAULT_APP"; then
        log_error "Invalid app: $DEFAULT_APP"
        log_error "Available apps: ${AVAILABLE_APPS[*]}"
        exit 1
    fi

    build_image "$DEFAULT_APP" "$DEFAULT_TAG" "$COMMIT_HASH"
fi

log_success "Docker build completed! 🎉"