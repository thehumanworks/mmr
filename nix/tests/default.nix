# Flake checks. The package check runs the Rust test suite; the module checks
# build real home-manager and NixOS configurations and inspect what they write.
{
  pkgs,
  self,
  nixpkgs,
  home-manager,
}:

let
  inherit (pkgs.stdenv.hostPlatform) isLinux system;
in
{
  package = self.packages.${system}.mmr;

  home-manager = import ./home-manager.nix { inherit pkgs self home-manager; };
}
// pkgs.lib.optionalAttrs isLinux {
  nixos-module = import ./nixos-module.nix { inherit pkgs self nixpkgs; };

  # Boots a VM; needs a builder with the `kvm` system feature.
  nixos-vm = import ./nixos-vm.nix { inherit pkgs self; };
}
