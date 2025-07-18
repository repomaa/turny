{
  description = "Turny Spotify RFID Controller Development Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # System dependencies for librespot and audio
            alsa-lib
            alsa-lib.dev
            pkg-config
            openssl
            openssl.dev

            # Additional dependencies for Raspberry Pi GPIO and SPI
            # (these might not be needed on x86_64 but won't hurt)
            udev

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
          PKG_CONFIG_PATH = "${pkgs.alsa-lib.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig";
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

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "turny";
          version = "0.1.0";

          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            cmake
            rustPlatform.bindgenHook
          ];

          buildInputs = with pkgs; [
            alsa-lib
            alsa-lib.dev
            openssl
            openssl.dev
            udev
          ];

          meta = with pkgs.lib; {
            description = "Turny Spotify RFID Controller (native build)";
            homepage = "https://github.com/user/turny";
            license = licenses.mit;
            maintainers = [ ];
            platforms = platforms.linux;
          };
        };

        packages.cross-rpi =
          let
            crossPkgs = pkgs.pkgsCross.aarch64-multiplatform.pkgsStatic;
          in
          crossPkgs.rustPlatform.buildRustPackage {
            pname = "turny";
            version = "0.1.0";

            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = with crossPkgs; [
              pkg-config
              cmake
              rustPlatform.bindgenHook
            ];

            buildInputs = with crossPkgs; [
              alsa-lib
              alsa-lib.dev
              openssl
              openssl.dev
              udev
            ];

            meta = with pkgs.lib; {
              description = "Turny Spotify RFID Controller";
              homepage = "https://github.com/user/turny";
              license = licenses.mit;
              maintainers = [ ];
              platforms = [
                "aarch64-linux"
              ];
            };
          };
      }
    );
}
