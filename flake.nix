{
  description = "mmr: browse local AI coding-session history from Claude Code, Codex, Cursor, Grok and Pi";

  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz";

    # Only used by `checks` to exercise the home-manager module.
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
    }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: {
        mmr = pkgs.callPackage ./nix/package.nix { };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.mmr;
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.mmr;
          meta.description = "Run the mmr CLI";
        };
      });

      overlays.default = final: _prev: {
        mmr = final.callPackage ./nix/package.nix { };
      };

      nixosModules = {
        mmr = ./nix/modules/nixos.nix;
        default = self.nixosModules.mmr;
      };

      homeModules = {
        mmr = ./nix/modules/home-manager.nix;
        default = self.homeModules.mmr;
      };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.mmr ];
          # `zsh` is needed by `cargo test`; `python3` runs `examples/`.
          packages = with pkgs; [
            clippy
            rustfmt
            rust-analyzer
            just
            python3
            zsh
          ];
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);

      checks = forAllSystems (
        pkgs:
        import ./nix/tests {
          inherit
            pkgs
            self
            nixpkgs
            home-manager
            ;
        }
      );
    };
}
