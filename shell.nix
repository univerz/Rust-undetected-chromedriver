with import <nixpkgs> {
};
mkShell {
  NIX_LD_LIBRARY_PATH = lib.makeLibraryPath [
    stdenv.cc.cc
    openssl
    glib
    glibc
    zlib
    fuse3
    nspr
    icu
    zlib
    nss
    curl
    expat
    xorg.libxcb
  ];
  NIX_LD = lib.fileContents "${stdenv.cc}/nix-support/dynamic-linker";
}
