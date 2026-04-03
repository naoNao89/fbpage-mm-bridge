# Build Caching Issue

## Problem

GitHub Actions workflow builds are extremely slow due to lack of Docker layer caching.

**Current state:**
- `dev/**` branches: Local build (`docker compose build`) — no caching, compiles from scratch every time
- `main` branch: Pulls from GHCR — no layer caching between builds

**Build times observed:**
- Rust services: ~5-10 minutes per build
- Full rebuild: 15+ minutes

## Root Cause

1. **No Docker layer caching** in GitHub Actions workflow
2. **No Rust/Cargo caching** — dependencies recompile every build
3. Each deploy rebuilds all layers even if only one service changed

## Proposed Solutions

### Option 1: GitHub Actions Docker Layer Cache
Add cache action to persist Docker layers between runs.

```yaml
- uses: docker/build-push-action@v5
  with:
    cache-from: type=gha
    cache-to: type=gha,mode=max
```

### Option 2: Cargo/Crate Caching
Cache Rust compilation artifacts.

```yaml
- name: Cache cargo
  uses: actions/cache@v3
  with:
    path: |
      ~/.cargo/bin/
      ~/.cargo/registry/index/
      ~/.cargo/registry/cache/
      ~/.cargo/git/
      target/
```

### Option 3: Split Build/Deploy Workflow
Separate build step that pushes to GHCR, then simple deploy that pulls.

- Build only on `main` pushes to GHCR
- `dev/**` branches use existing GHCR image + hot reload / exec

### Option 4: Docker Compose Optimization
Use `build.target` for multi-stage builds to reduce layer size and improve caching.

## Recommended Approach

Combine **Option 1 + Option 2**:
1. Add Docker layer caching via `docker/build-push-action` with GitHub Actions cache backend
2. Add Cargo caching for Rust services
3. Target: <3 minute build times for incremental changes

## Files to Modify

- `.github/workflows/deploy-mattermost.yml`
- Potentially `services/*/Dockerfile` for multi-stage builds

## Priority

High — blocks developer productivity during feature development.
