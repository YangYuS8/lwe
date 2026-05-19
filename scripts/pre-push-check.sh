#!/usr/bin/env bash
# Pre-push quality check script
# Mimics CI checks but runs locally for faster feedback

set -e # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print section headers
print_section() {
	echo -e "\n${BLUE}==>${NC} ${1}"
}

# Function to print success
print_success() {
	echo -e "${GREEN}✓${NC} ${1}"
}

# Function to print error
print_error() {
	echo -e "${RED}✗${NC} ${1}"
}

# Function to print warning
print_warning() {
	echo -e "${YELLOW}⚠${NC} ${1}"
}

# Start timer
START_TIME=$(date +%s)

echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║   LWE Pre-Push Quality Checks          ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"

# 1. Frontend type checks
print_section "Running frontend type checks..."
if pnpm check; then
    print_success "Frontend type checks passed"
else
    print_error "Frontend type checks failed"
    exit 1
fi

# 2. Frontend tests
print_section "Running frontend tests..."
if pnpm test; then
	print_success "Frontend tests passed"
else
	print_error "Frontend tests failed"
	exit 1
fi

# 3. Frontend production build
print_section "Building frontend..."
if pnpm build; then
    print_success "Frontend build passed"
else
    print_error "Frontend build failed"
    exit 1
fi

# 4. Documentation build
print_section "Building documentation..."
if pnpm docs:build; then
	print_success "Documentation build passed"
else
	print_error "Documentation build failed"
	exit 1
fi

# 5. Format check
print_section "Checking code formatting..."
if cargo fmt --all -- --check >/dev/null 2>&1; then
	print_success "Code formatting is correct"
else
	print_error "Code formatting failed"
	echo "Run: cargo fmt --all"
	exit 1
fi

# 6. Clippy (workspace mode)
print_section "Running Clippy (workspace)..."
if cargo clippy --workspace --all-targets -- -D warnings; then
	print_success "Clippy checks passed"
else
	print_error "Clippy found issues"
	echo "Run: cargo clippy --workspace --all-targets -- -D warnings"
	exit 1
fi

# 7. Cargo check (workspace)
print_section "Checking compilation (workspace)..."
if cargo check --workspace --quiet >/dev/null 2>&1; then
	print_success "Workspace compiles"
else
	print_error "Workspace failed to compile"
	exit 1
fi

# 8. Tests (workspace)
if [ "${SKIP_TESTS:-0}" = "0" ]; then
	print_section "Running tests (workspace)..."
	if cargo test --workspace --quiet >/dev/null 2>&1; then
		print_success "Tests passed"
	else
		print_error "Tests failed"
		echo "Run: cargo test --workspace"
		exit 1
	fi
else
	print_warning "Tests skipped (SKIP_TESTS=1)"
fi

# Calculate elapsed time
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo -e "\n${GREEN}╔════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  All checks passed! ✓                  ║${NC}"
echo -e "${GREEN}║  Time: ${ELAPSED}s                               ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════╝${NC}"
echo -e "\n${BLUE}Ready to push!${NC}\n"
