export default async function () {
  const base = "https://github.com/tangramdotdev/bootstrap/releases/download/v2026.09.16/";
  const [toolchain, sdk] = await Promise.all([
    tg.download(base + "toolchain_aarch64_darwin.tar.zst", "sha256:917cbc227e5c2b04229a02239ce6dc42bdcfeb56d87f1ed9eda96cd8bf340b52", { mode: "extract" }),
    tg.download(base + "macos_sdk_27.0.tar.zst", "sha256:fea5b356d1a7480d459d9f40f6945823a83dc17f8fa16f58f1fba15b34e4c9ed", { mode: "extract" }),
  ]);
  const source = tg.file('#include <stdio.h>\nint main(void) { puts("Tangram managed native macOS build succeeded"); return 0; }\n');
  return tg.build({
    executable: { artifact: toolchain, path: "bin/clang" },
    args: ["-x", "c", "-isysroot", sdk, source, "-o", tg.output],
    env: { PATH: tg`${toolchain}/bin` },
  });
}
