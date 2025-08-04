{
  description = "Tool to wrap the BitRipple tunnel into another tunnel";

  inputs = {
    nixpkgs.url = "nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    axlrust.url = "git+ssh://git@github.com/BitRipple-Inc/AxlRust.git";
    axlrust.flake = false;
    axl = {
      url = "git+ssh://git@github.com/BitRipple-Inc/Axl.git";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    csnip = {
      url = "git+ssh://git@github.com/lorinder/csnip.git";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, axlrust, axl, csnip }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
      };
    in {
      packages = {
        default = pkgs.rustPlatform.buildRustPackage rec {
          name = "tunnel_inserter";
          version = "0.1.0";
          src = ./.;
          postPatch = ''
            ln -s ${axlrust} ../AxlRust
          '';
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = with pkgs; [
            lsof
            clang
            llvmPackages.libclang
            pkgconf
          ];

          buildInputs = [
            axl.outputs.packages.${system}.default
            csnip.outputs.packages.${system}.default
          ];

          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        };
      };

      devShells = {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
          ];
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          shellHook = ''
            export LD_LIBRARY_PATH=${pkgs.llvmPackages.libclang.lib}/lib:$LD_LIBRARY_PATH
          '';
        };
      };
    });
}

# vim:sw=2:sts=2:et
