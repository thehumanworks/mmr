# Only needed for testing Pixi packages on NixOS. Ordinary FHS Linux does not use this adapter.
let pkgs = import <nixpkgs> {}; in pkgs.buildFHSEnv {
  name = "runner-evaluation-fhs";
  targetPkgs = p: [ p.python3 p.bash p.coreutils p.git p.curl p.glibc p.zlib p.stdenv.cc.cc.lib ];
  runScript = "bash";
}
