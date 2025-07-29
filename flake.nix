{
  description = "Tool to wrap the BitRipple tunnel into another tunnel";

  inputs = {
    nixpkgs.url = "nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
    axlrust.url = "git+ssh//:git@github.com/BitRipple-Inc/AxlRust.git";
    axlrust.flake = false;
  };

  outputs = { self, nixpkgs, flake-utils, axlrust }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
      };
    in {
      packages = {
        default = pkgs.rustPlatform.buildRustPackage rec {
          name = "tunnel_inserter";
          # version = 0.1.0;
          src = ./.;
          postPatch = ''
            ln -s ${axlrust} ../AxlRust
          '';
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = with pkgs; [
            lsof
          ];
	};
      };
    });
}

# vim:sw=2:sts=2:et
