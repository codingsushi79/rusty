{
  description = "Rusty — a fast, friendly terminal code editor written in Rust";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));

      package = pkgs: pkgs.rustPlatform.buildRustPackage {
        pname = "rusty";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.openssl ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.libxcb pkgs.libxkbcommon ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin
            (with pkgs.darwin.apple_sdk.frameworks; [ Security SystemConfiguration AppKit ]);
        meta = {
          description = "A terminal code editor written in Rust";
          mainProgram = "rusty";
        };
      };
    in {
      packages = forAll (pkgs: { default = package pkgs; });

      apps = forAll (pkgs: {
        default = {
          type = "app";
          program = "${package pkgs}/bin/rusty";
        };
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          buildInputs = with pkgs; [ rustc cargo rustfmt clippy pkg-config openssl ];
        };
      });
    };
}
