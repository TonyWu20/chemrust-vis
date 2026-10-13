{
  description = "rushi development shell";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    devshell.url = "github:numtide/devshell";
  };
  outputs = { nixpkgs, devshell, fenix, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-darwin" ];

      pkgsFor = system: import nixpkgs {
        config.allowUnfree = true;
        inherit system; overlays = [
        devshell.overlays.default
        fenix.overlays.default
      ];
      };
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          rustPackages = (fenix.packages.${system}.stable.withComponents [
            "cargo"
            "clippy"
            "rust-src"
            "rustc"
            "rustfmt"
            "rust-analyzer"
          ]);
        in
        {
          default = pkgs.devshell.mkShell {
            packages = with pkgs; [
              rustPackages
              mdfried
              pkg-config
              libcxx
              gcc
            ] ++ lib.optionals stdenv.isDarwin [
              pkgs.apple-sdk_26
              pkgs.libiconv
            ];
            env = with pkgs;lib.optionals stdenv.isDarwin [
              {
                name = "RUSTFLAGS";
                value = "-C link-arg=-L${pkgs.libiconv}/lib";
              }
            ];
          };
        }
      );
    };
}
