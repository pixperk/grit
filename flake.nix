{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      forAllSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    in {
      devShells = forAllSystems (system:
        let
          overlays = [ rust-overlay.overlays.default ];
          pkgs = import nixpkgs { inherit system overlays; };
          rust = pkgs.rust-bin.stable.latest.default;
        in {
          default = pkgs.mkShell {
            buildInputs = [
              rust
              pkgs.mpv
              pkgs.yt-dlp
            ];
          };
        }
      );

      packages = forAllSystems (system:
        let
          overlays = [ rust-overlay.overlays.default ];
          pkgs = import nixpkgs { inherit system overlays; };
        in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "grit";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.makeWrapper ];
            buildInputs = [ pkgs.mpv pkgs.yt-dlp ];
            postInstall = ''
              wrapProgram $out/bin/grit --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.mpv pkgs.yt-dlp ]}
            '';
          };
        }
      );
    };
}
