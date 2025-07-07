{
  description = "Turny Spotify RFID Controller Development Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
          ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            rustToolchain

            # System dependencies for librespot and audio
            alsa-lib
            alsa-lib.dev
            pkg-config
            openssl
            openssl.dev

            # Avahi for DNS-SD/mDNS discovery (required by librespot)
            avahi
            avahi.dev

            # Additional dependencies for Raspberry Pi GPIO and SPI
            # (these might not be needed on x86_64 but won't hurt)
            libudev-zero

            # Development tools
            cargo-watch
            cargo-edit

            # For debugging
            gdb
            valgrind

            # Network tools for testing
            curl

            # Audio tools for testing
            alsa-utils
          ];

          # Environment variables
          RUST_BACKTRACE = "1";
          PKG_CONFIG_PATH = "${pkgs.alsa-lib.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.avahi.dev}/lib/pkgconfig";
          ALSA_PCM_CARD = "default";
          ALSA_PCM_DEVICE = "0";

          # Shell hook
          # shellHook = ''
          #   echo "🎵 Turny Spotify RFID Controller Development Environment"
          #   echo "Rust version: $(rustc --version)"
          #   echo "Cargo version: $(cargo --version)"
          #   echo ""
          #   echo "Available commands:"
          #   echo "  cargo build --release    # Build the project"
          #   echo "  cargo run                # Run the project"
          #   echo "  cargo test               # Run tests"
          #   echo "  cargo watch -x run       # Watch for changes and run"
          #   echo ""
          #   echo "ALSA libraries available at: ${pkgs.alsa-lib}"
          #   echo "OpenSSL libraries available at: ${pkgs.openssl}"
          #   echo ""
          #
          #   # Check if we're on a Raspberry Pi
          #   if [ -d "/sys/class/gpio" ]; then
          #     echo "🔌 GPIO detected - Raspberry Pi environment ready"
          #   else
          #     echo "⚠️  No GPIO detected - running in development mode"
          #   fi
          # '';
        };
      }
    );
}
