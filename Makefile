# OxideMX - Build System
#
# Usage:
#   make build            - Build the Rust daemon
#   make builtin-widgets  - Pack the bundled built-in widgets (.omxw)
#   make clean            - Clean build artifacts
#   make run              - Run OxideMX (daemon + overlay)

.PHONY: all build builtin-widgets clean run help

# Default target
all: build

# Build Rust daemon
build:
	@echo "Building Rust daemon..."
	cd daemon && cargo build --release
	@echo "✓ Daemon built: daemon/target/release/oxidemxd"

# Build + pack the bundled built-in widgets into target/builtin-widgets/
# (requires the wasm target: rustup target add wasm32-wasip1).
# install.sh ships these to <prefix>/share/oxidemx/widgets for startup
# seeding (spec §16).
builtin-widgets:
	./scripts/build-builtin-widgets.sh

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cd daemon && cargo clean
	@echo "✓ Clean complete"

# Run OxideMX
run: build
	@echo "Starting OxideMX..."
	./scripts/oxidemx.sh

# Help
help:
	@echo "OxideMX Build System"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build            - Build the Rust daemon (default)"
	@echo "  builtin-widgets  - Pack bundled built-in widgets (.omxw)"
	@echo "  clean            - Clean build artifacts"
	@echo "  run              - Build and run OxideMX"
	@echo "  help             - Show this help"
