#!/bin/bash
# Build and optionally push Apollo MCP Server Docker image
# Uses git commit hash + date for build identification

set -e

# Load environment if available
if [ -f .env ]; then
    source .env
fi

# Git-based version info
GIT_HASH=$(git rev-parse --short HEAD)
GIT_BRANCH=$(git rev-parse --abbrev-ref HEAD | sed 's/[^a-zA-Z0-9]/-/g')
BUILD_DATE=$(date +%Y%m%d)
BUILD_TIMESTAMP=$(date +%Y%m%d-%H%M%S)

# Cargo version from Cargo.toml
CARGO_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')

# Build tag formats
TAG_LATEST="latest"
TAG_VERSION="${CARGO_VERSION}"
TAG_BUILD="${BUILD_DATE}-${GIT_HASH}"
TAG_FULL="${CARGO_VERSION}-${BUILD_DATE}-${GIT_HASH}"

# Registry settings (from .env or defaults)
CONTAINER_REPO="${CONTAINER_REPO:-apollo-mcp-server}"

echo "=============================================="
echo "Apollo MCP Server - Docker Build"
echo "=============================================="
echo "Cargo Version : ${CARGO_VERSION}"
echo "Git Commit    : ${GIT_HASH}"
echo "Git Branch    : ${GIT_BRANCH}"
echo "Build Date    : ${BUILD_DATE}"
echo "Build Tags    :"
echo "  - ${TAG_LATEST}"
echo "  - ${TAG_VERSION}"
echo "  - ${TAG_BUILD}"
echo "  - ${TAG_FULL}"
echo "=============================================="

# Build the image with multiple tags
echo ""
echo "Building Docker image..."
docker build \
    -t apollo-mcp-server:${TAG_LATEST} \
    -t apollo-mcp-server:${TAG_VERSION} \
    -t apollo-mcp-server:${TAG_BUILD} \
    -t apollo-mcp-server:${TAG_FULL} \
    --build-arg BUILD_DATE="${BUILD_TIMESTAMP}" \
    --build-arg GIT_HASH="${GIT_HASH}" \
    --build-arg VERSION="${CARGO_VERSION}" \
    .

echo ""
echo "Build complete!"
echo ""
echo "Local images created:"
docker images | grep apollo-mcp-server | head -10

# Push if requested
if [ "$1" == "--push" ] || [ "$1" == "-p" ]; then
    echo ""
    echo "=============================================="
    echo "Pushing to registry: ${CONTAINER_REPO}"
    echo "=============================================="

    if [ -z "$GITHUB_PAT" ]; then
        echo "Error: GITHUB_PAT not set. Run: source .env"
        exit 1
    fi

    # Login to registry
    echo "$GITHUB_PAT" | docker login ghcr.io -u ${GITHUB_USERNAME:-bwkiwi} --password-stdin

    # Tag and push
    for TAG in ${TAG_LATEST} ${TAG_VERSION} ${TAG_BUILD} ${TAG_FULL}; do
        echo "Pushing ${CONTAINER_REPO}:${TAG}..."
        docker tag apollo-mcp-server:${TAG} ${CONTAINER_REPO}:${TAG}
        docker push ${CONTAINER_REPO}:${TAG}
    done

    echo ""
    echo "Push complete!"
    echo ""
    echo "Images available at:"
    echo "  - ${CONTAINER_REPO}:${TAG_LATEST}"
    echo "  - ${CONTAINER_REPO}:${TAG_VERSION}"
    echo "  - ${CONTAINER_REPO}:${TAG_BUILD}"
    echo "  - ${CONTAINER_REPO}:${TAG_FULL}"
fi

echo ""
echo "Done! Build tag: ${TAG_BUILD}"
