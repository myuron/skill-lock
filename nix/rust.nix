{
  lib,
  makeRustPlatform,
  makeWrapper,
  rust-bin,
  git,
}:
let
  toolchain = rust-bin.stable.latest.default;
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "skill-lock";
  version = "0.1.0";
  src = ../.;
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [ makeWrapper ];
  # テストでローカルリポジトリを作成・取得するため
  nativeCheckInputs = [ git ];
  # skill の取得に git CLI を使うため
  postInstall = ''
    wrapProgram $out/bin/skill-lock --prefix PATH : ${lib.makeBinPath [ git ]}
  '';
}
