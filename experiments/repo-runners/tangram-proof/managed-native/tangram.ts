import * as std from "../packages/packages/std";
export default async function () {
  const toolchain = await std.bootstrap.toolchain();
  const sdk = await std.bootstrap.macOsSdk();
  const source = tg.file('#include <stdio.h>\nint main(void) { puts("Tangram managed native macOS build succeeded"); return 0; }\n');
  return tg.build({
    executable: { artifact: toolchain, path: "bin/clang" },
    args: ["-x", "c", "-isysroot", tg`${sdk}/MacOSX.sdk`, source, "-o", tg.output],
    env: { PATH: tg`${toolchain}/bin` },
  });
}

export async function standardSdk() {
  const sdk = await std.sdk(await std.bootstrap.sdk.arg());
  const source = tg.file('#include <stdio.h>\nint main(void) { puts("Tangram std.sdk macOS build succeeded"); return 0; }\n');
  return tg.build({
    executable: "cc",
    args: ["-x", "c", source, "-o", tg.output],
    env: await std.env.compose(sdk),
  });
}
