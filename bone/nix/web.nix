{
  lib,
  stdenv,
  nodejs,
  pnpm,
  fetchPnpmDeps,
  pnpmConfigHook,
}:

stdenv.mkDerivation (finalAttrs: {
  pname = "bone-web";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../web;
    fileset = lib.fileset.unions [
      ../web/index.html
      ../web/package.json
      ../web/pnpm-lock.yaml
      ../web/src
      ../web/tsconfig.json
      ../web/vite.config.ts
    ];
  };

  nativeBuildInputs = [
    nodejs
    pnpm
    pnpmConfigHook
  ];

  pnpmDeps = fetchPnpmDeps {
    inherit (finalAttrs) pname version src;
    inherit pnpm;
    fetcherVersion = 4;
    hash = "sha256-i44xJsDTmI1S1gtjfYCOpeFjVUhBtR6Ny5z3o+UcSHg=";
  };

  buildPhase = ''
    runHook preBuild
    pnpm build
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    cp -r dist $out
    runHook postInstall
  '';
})
