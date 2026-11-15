"""Buck2-owned Cargo build action; intentionally not a native Rust rule graph."""

def _build(ctx):
    binary = ctx.actions.declare_output("mmr")
    ctx.actions.run(
        cmd_args(
            "/bin/sh",
            ctx.attrs.script,
            binary.as_output(),
            ctx.attrs.source,
        ),
        env = {"PATH": ctx.attrs.tool_path},
        category = "cargo_source_build",
        local_only = True,
    )
    return [DefaultInfo(default_output = binary), RunInfo(args = cmd_args(binary))]

cargo_application = rule(
    impl = _build,
    attrs = {
        "source": attrs.source(),
        "tool_path": attrs.string(),
        "script": attrs.source(),
    },
)
