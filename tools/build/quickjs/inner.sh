#!/usr/bin/env bash
# Runs inside the container: /src holds the verified tarballs, /out receives the binaries.
set -euo pipefail

work=$(mktemp -d)
cd "$work"

tar xf /src/quickjs-"$QJS_VERSION".tar.xz
make -C quickjs-"$QJS_VERSION" -j"$(nproc)" CONFIG_WIN32=y LDFLAGS=-static qjs.exe
x86_64-w64-mingw32-strip -o /out/qjs.exe quickjs-"$QJS_VERSION"/qjs.exe

# The Linux twin runs the same engine on the host for control runs.
make -C quickjs-"$QJS_VERSION" clean >/dev/null
make -C quickjs-"$QJS_VERSION" -j"$(nproc)" qjs
strip -o /out/qjs-linux quickjs-"$QJS_VERSION"/qjs

tar xf /src/quickjs-ng-"$QJS_NG_VERSION".tar.gz
cmake -S quickjs-"$QJS_NG_VERSION" -B ng -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_SYSTEM_NAME=Windows -DCMAKE_C_COMPILER=x86_64-w64-mingw32-gcc \
  -DCMAKE_EXE_LINKER_FLAGS=-static >/dev/null
cmake --build ng -j"$(nproc)" --target qjs_exe >/dev/null
x86_64-w64-mingw32-strip -o /out/qjs-ng.exe ng/qjs.exe

for exe in /out/qjs.exe /out/qjs-ng.exe; do
    echo "$(basename "$exe") imports: $(x86_64-w64-mingw32-objdump -p "$exe" | sed -n 's/^\s*DLL Name: //p' | sort -u | tr '\n' ' ')"
done
