# OxideMX MX - Build System
#
# Usage:
#   make build   - Build the Rust daemon
#   make clean   - Clean build artifacts
#   make run     - Run OxideMX MX (daemon + overlay)

.PHONY: all build clean run help

# Default target
all: build

# Build Rust daemon
build:
	@echo "Building Rust daemon..."
	cd daemon && cargo build --release
	@echo "✓ Daemon built: daemon/target/release/oxidemxd"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cd daemon && cargo clean
	@echo "✓ Clean complete"

# Run OxideMX MX
run: build
	@echo "Starting OxideMX MX..."
	./scripts/oxidemx.sh

# Help
help:
	@echo "OxideMX MX Build System"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build  - Build the Rust daemon (default)"
	@echo "  clean  - Clean build artifacts"
	@echo "  run    - Build and run OxideMX MX"
	@echo "  help   - Show this help"
