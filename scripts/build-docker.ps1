# Build and optionally push Apollo MCP Server Docker image
# Uses git commit hash + date for build identification
#
# Usage:
#   .\scripts\build-docker.ps1          # Build only
#   .\scripts\build-docker.ps1 -Push    # Build and push to registry

param(
    [switch]$Push
)

$ErrorActionPreference = "Stop"

# Load environment if available
$envFile = ".\.env"
if (Test-Path $envFile) {
    Get-Content $envFile | ForEach-Object {
        if ($_ -match '^([^#][^=]+)=(.*)$') {
            $name = $matches[1].Trim()
            $value = $matches[2].Trim()
            Set-Item -Path "env:$name" -Value $value
        }
    }
}

# Git-based version info
$GIT_HASH = (git rev-parse --short HEAD).Trim()
$GIT_BRANCH = (git rev-parse --abbrev-ref HEAD).Trim() -replace '[^a-zA-Z0-9]', '-'
$BUILD_DATE = Get-Date -Format "yyyyMMdd"
$BUILD_TIMESTAMP = Get-Date -Format "yyyyMMdd-HHmmss"

# Cargo version from Cargo.toml
$CARGO_VERSION = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=' | Select-Object -First 1).Line -replace '.*"(.*)".*', '$1'

# Build tag formats
$TAG_LATEST = "latest"
$TAG_VERSION = $CARGO_VERSION
$TAG_BUILD = "${BUILD_DATE}-${GIT_HASH}"
$TAG_FULL = "${CARGO_VERSION}-${BUILD_DATE}-${GIT_HASH}"

# Registry settings (from .env or defaults)
$CONTAINER_REPO = if ($env:CONTAINER_REPO) { $env:CONTAINER_REPO } else { "apollo-mcp-server" }

Write-Host "=============================================="
Write-Host "Apollo MCP Server - Docker Build"
Write-Host "=============================================="
Write-Host "Cargo Version : $CARGO_VERSION"
Write-Host "Git Commit    : $GIT_HASH"
Write-Host "Git Branch    : $GIT_BRANCH"
Write-Host "Build Date    : $BUILD_DATE"
Write-Host "Build Tags    :"
Write-Host "  - $TAG_LATEST"
Write-Host "  - $TAG_VERSION"
Write-Host "  - $TAG_BUILD"
Write-Host "  - $TAG_FULL"
Write-Host "=============================================="

# Build the image with multiple tags
Write-Host ""
Write-Host "Building Docker image..."
docker build `
    -t "apollo-mcp-server:$TAG_LATEST" `
    -t "apollo-mcp-server:$TAG_VERSION" `
    -t "apollo-mcp-server:$TAG_BUILD" `
    -t "apollo-mcp-server:$TAG_FULL" `
    --build-arg "BUILD_DATE=$BUILD_TIMESTAMP" `
    --build-arg "GIT_HASH=$GIT_HASH" `
    --build-arg "VERSION=$CARGO_VERSION" `
    .

if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed!" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "Build complete!" -ForegroundColor Green
Write-Host ""
Write-Host "Local images created:"
docker images | Select-String "apollo-mcp-server" | Select-Object -First 10

# Push if requested
if ($Push) {
    Write-Host ""
    Write-Host "=============================================="
    Write-Host "Pushing to registry: $CONTAINER_REPO"
    Write-Host "=============================================="

    if (-not $env:GITHUB_PAT) {
        Write-Host "Error: GITHUB_PAT not set. Create .env file with credentials." -ForegroundColor Red
        exit 1
    }

    $GITHUB_USERNAME = if ($env:GITHUB_USERNAME) { $env:GITHUB_USERNAME } else { "bwkiwi" }

    # Login to registry
    $env:GITHUB_PAT | docker login ghcr.io -u $GITHUB_USERNAME --password-stdin

    if ($LASTEXITCODE -ne 0) {
        Write-Host "Login failed!" -ForegroundColor Red
        exit 1
    }

    # Tag and push each version
    $tags = @($TAG_LATEST, $TAG_VERSION, $TAG_BUILD, $TAG_FULL)
    foreach ($tag in $tags) {
        Write-Host "Pushing ${CONTAINER_REPO}:${tag}..."
        docker tag "apollo-mcp-server:$tag" "${CONTAINER_REPO}:${tag}"
        docker push "${CONTAINER_REPO}:${tag}"

        if ($LASTEXITCODE -ne 0) {
            Write-Host "Push failed for tag: $tag" -ForegroundColor Red
            exit 1
        }
    }

    Write-Host ""
    Write-Host "Push complete!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Images available at:"
    foreach ($tag in $tags) {
        Write-Host "  - ${CONTAINER_REPO}:${tag}"
    }
}

Write-Host ""
Write-Host "Done! Build tag: $TAG_BUILD" -ForegroundColor Cyan
