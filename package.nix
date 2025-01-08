{
  pkg-config,
  darwin,
  lib,
  libiconv,
  craneLib,
  rustToolchain,
  system,
}:
let
  isDarwin = lib.strings.hasSuffix "-darwin" system;
  commonInputs = {
    src = craneLib.cleanCargoSource (craneLib.path ./.);
    strictDeps = true;
    nativeBuildInputs =
      [
        rustToolchain
        pkg-config
      ]
      ++ lib.optional isDarwin [
        darwin.apple_sdk.frameworks.SystemConfiguration
        libiconv
      ];
  };
in
craneLib.buildPackage commonInputs
// {
  cargoArtifacts = craneLib.buildDepsOnly commonInputs;
}
