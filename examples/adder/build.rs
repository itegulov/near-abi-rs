use near_abi_rs::Config;

fn main() -> anyhow::Result<()> {
    let config = Config { out_dir: None };
    config.compile_abi(&["src/adder-metadata.json"])?;
    Ok(())
}
