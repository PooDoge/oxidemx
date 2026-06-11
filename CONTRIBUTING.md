# Contributing to OxideMX

First off, thank you for considering contributing to OxideMX! It's people like you that make this project better for everyone.

## Code of Conduct

By participating in this project, you agree to maintain a welcoming, inclusive, and harassment-free environment. Please be respectful and constructive in all interactions.

## How Can I Contribute?

### Reporting Bugs

Before creating a bug report, please check the [existing issues](https://github.com/PooDoge/oxidemx/issues) to avoid duplicates.

When reporting a bug, include:

- **Clear title** describing the issue
- **Steps to reproduce** the behavior
- **Expected behavior** vs **actual behavior**
- **System information**:
  - Linux distribution and version
  - Desktop environment (KDE Plasma version)
  - Logitech mouse model
  - OxideMX version

### Suggesting Features

Feature requests are welcome! Please:

1. Check existing issues/discussions first
2. Describe the problem your feature would solve
3. Propose your solution
4. Consider alternatives you've thought about

### Pull Requests

1. **Fork** the repository
2. **Create a branch** from `master`:
   ```bash
   git checkout -b feature/your-feature-name
   ```
3. **Make your changes** following our code style
4. **Test thoroughly** on your system
5. **Commit** with clear messages:
   ```bash
   git commit -m "feat: add new radial menu animation"
   ```
6. **Push** and create a Pull Request

## Development Setup

### Prerequisites

- Rust (latest stable)
- GTK4 / Libadwaita development headers (for settings-rs)
- Wayland / X11 development headers

### System Dependencies

**Fedora:**
```bash
sudo dnf install \
  rust cargo \
  dbus-devel systemd-devel \
  libevdev-devel hidapi-devel \
  gtk4-devel libadwaita-devel \
  git make
```

**Arch Linux:**
```bash
sudo pacman -S --needed \
  rust \
  dbus systemd-libs \
  libevdev hidapi \
  gtk4 libadwaita \
  git make base-devel
```

**Debian/Ubuntu:**
```bash
sudo apt install \
  rustc cargo \
  libdbus-1-dev libsystemd-dev \
  libevdev-dev libhidapi-dev \
  libgtk-4-dev libadwaita-1-dev \
  git make build-essential
```

### Building

```bash
# Clone the repository
git clone https://github.com/PooDoge/oxidemx
cd oxidemx

# Build the entire workspace
cargo build --release
```

### Running Locally

```bash
# Start the daemon in verbose mode
./target/release/oxidemxd --verbose

# In another terminal, run the overlay
./target/release/oxidemx-overlay

# Or run the settings app
./target/release/oxidemx-settings
```

### Testing

```bash
# Run tests
cargo test --all

# Lint checks
cargo clippy --all
```

## Code Style

### Rust (Workspace)

- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` before committing
- Run `cargo clippy` and address warnings
- Document public APIs with `///` doc comments

## Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style (formatting, no logic change)
- `refactor`: Code refactoring
- `test`: Adding/updating tests
- `chore`: Maintenance tasks

Examples:
```
feat(overlay): add glassmorphic blur effect
fix(daemon): correct battery percentage parsing
docs: update installation instructions
```

## Project Structure

```
oxidemx/
├── daemon/           # Rust daemon (input handling, D-Bus, HID++)
├── overlay-rs/       # Rust + iced radial menu overlay
├── settings-rs/      # Rust + iced settings dashboard
├── popup-rs/         # Rust GJS-like quick indicator popup
├── oxidemx-shared/   # Shared types and config structures
├── oxidemx-icons/    # Icon rendering & assets
├── oxidemx-widgets/  # Custom UI widgets
├── oxidemx-window/   # Window and cursor management
├── assets/           # Theme backgrounds, images, and visual assets
└── packaging/        # Distribution files (systemd, udev rules)
```

## Getting Help

- **Questions**: Open a [Discussion](https://github.com/PooDoge/oxidemx/discussions)
- **Bugs**: Open an [Issue](https://github.com/PooDoge/oxidemx/issues)

## Recognition

Contributors will be recognized in:
- The project README
- Release notes

Thank you for helping make OxideMX better!

---

*PooDoge — based on JuhRadial MX by JuhLabs (Julian Hermstad)*
